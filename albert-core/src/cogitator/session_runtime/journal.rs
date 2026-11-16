use octo_core::Envelope;
use octo_history::{AppendReceipt, AppendRequest, SessionId};
use serde_json::to_vec;
use sha2::{Digest, Sha256};
use tracing::warn;

use crate::cogitator::{
    channel_of, context::UserInput, native_requests::NativeDecision,
    native_requests::NativeRequest, AlbertCogitator,
};
use crate::{
    composer::InputDigest, history::Turn, media::MediaInput, sessions::acceptance::AcceptedStart,
    sessions::acceptance::SourceReceipt, sessions::SessionContext,
};

use super::RequestPermit;
impl AlbertCogitator {
    pub(in crate::cogitator) async fn append_native(
        &self,
        incoming: &Envelope,
        input: &UserInput,
    ) -> Result<AppendReceipt, String> {
        let decision = incoming.context.get::<NativeDecision>().cloned();
        let native = match &decision {
            Some(decision) => decision.request.clone(),
            None => NativeRequest::from_envelope(incoming).map_err(|error| error.to_string())?,
        };
        let mut result = if incoming
            .context
            .get::<SessionContext>()
            .is_some_and(|session| !session.is_live())
        {
            Err("Session ended.".into())
        } else if let Some(binding) = decision
            .as_ref()
            .filter(|decision| !decision.request.aliases.is_empty())
            .and_then(|decision| decision.replay.as_ref())
        {
            Ok(binding.receipt.clone())
        } else {
            let mut digest = Sha256::new();
            digest.update(input.text.as_bytes());
            for image in &input.images {
                digest.update((image.bytes().len() as u64).to_be_bytes());
                digest.update(image.bytes());
            }
            let digest = incoming
                .context
                .get::<InputDigest>()
                .map(|value| value.0.clone())
                .or_else(|| incoming.context.get::<MediaInput>().map(MediaInput::digest))
                .unwrap_or_else(|| format!("{:x}", digest.finalize()));
            let request = native.append_request(
                digest,
                vec![Turn::user(self.media_transcript(incoming, input).await?)],
            );
            if native.aliases.is_empty() {
                self.history
                    .append_journal(&channel_of(incoming), &request)
                    .await
                    .map_err(|error| error.to_string())
            } else if let Some(history) = &self.session_history {
                history
                    .append_aliased(&channel_of(incoming), &request, &native.aliases)
                    .await
                    .map_err(|error| error.to_string())
            } else {
                Err("Native aliases require a session journal.".into())
            }
        };
        if let (Ok(receipt), Some(proof), Some(session), Some(service)) = (
            &mut result,
            incoming.context.get::<AcceptedStart>(),
            incoming.context.get::<SessionContext>(),
            self.session_service.as_ref(),
        ) {
            let digest = incoming
                .context
                .get::<InputDigest>()
                .map(|value| value.0.clone())
                .or_else(|| incoming.context.get::<MediaInput>().map(MediaInput::digest))
                .unwrap_or_else(|| format!("{:x}", Sha256::digest(input.text.as_bytes())));
            if receipt.duplicate
                && proof.consume(session.session_id().as_str(), &native.key, &digest)
            {
                match service
                    .web_run_start(session.session_id(), &native.key)
                    .await
                {
                    Ok(true) => receipt.duplicate = false,
                    Ok(false) => {}
                    Err(error) => return Err(error.to_string()),
                }
            }
        }
        if let Some(receipt) = incoming.context.get::<SourceReceipt>() {
            receipt.complete(result.clone());
        }
        if let Some(permit) = incoming.context.get::<RequestPermit>() {
            permit.lifecycle.lock().unwrap().take();
            permit.request.lock().unwrap().take();
        }
        result
    }
    pub(in crate::cogitator) async fn append_host(
        &self,
        key: &str,
        scope: &str,
        phase: &str,
        turns: Vec<Turn>,
    ) -> Result<(), String> {
        self.append_host_records(key, scope, phase, turns)
            .await
            .map(|_| ())
    }
    pub(in crate::cogitator) async fn append_host_records(
        &self,
        key: &str,
        scope: &str,
        phase: &str,
        turns: Vec<Turn>,
    ) -> Result<Vec<i64>, String> {
        if turns.is_empty() {
            return Ok(vec![]);
        }
        if self.session_history.is_none() {
            return self
                .history
                .append(key, &turns)
                .await
                .map(|_| vec![])
                .map_err(|error| error.to_string());
        }
        let bytes = to_vec(&turns).map_err(|error| error.to_string())?;
        let receipt = self
            .history
            .append_journal(
                key,
                &AppendRequest {
                    namespace: "albert/host".into(),
                    idempotency_key: format!("{scope}/{phase}"),
                    digest: format!("{:x}", Sha256::digest(bytes)),
                    turns,
                },
            )
            .await
            .map_err(|error| error.to_string())?;
        if matches!(phase, "final" | "reflex-result" | "confirmation-wait") {
            if let (Some(service), Some(raw)) =
                (&self.session_service, key.strip_prefix("session/"))
            {
                if let Ok(id) = SessionId::parse(raw) {
                    if let Err(error) = service.web_finish_start(&id).await {
                        warn!(%error,"first-run publication will reconcile on read");
                    }
                }
            }
        }
        Ok(receipt.record_ids)
    }
}
