use std::{sync::Arc, sync::Mutex, sync::Weak};

use octo_core::{CogitatorContext, Envelope};
use tokio::sync::Mutex as AsyncMutex;

use crate::cogitator::{
    native_requests::NativeDecision, native_requests::NativeRequest, AlbertCogitator,
};
use crate::{
    sessions::error::SessionError, sessions::error::SessionResult, sessions::ingress::for_message,
    sessions::SessionContext,
};

use super::RequestPermit;
impl AlbertCogitator {
    pub(in crate::cogitator) async fn admit_message(
        &self,
        incoming: Arc<Envelope>,
        ctx: &CogitatorContext,
    ) -> SessionResult<Arc<Envelope>> {
        let incoming = self.transport.ingress(incoming);
        let Some(service) = &self.session_service else {
            return Ok(incoming);
        };
        let stamped = incoming.context.get::<SessionContext>().cloned();
        if incoming.source.as_str() == "web"
            && stamped
                .as_ref()
                .is_none_or(|context| context.actor_tools.is_none())
        {
            return Err(SessionError::Denied);
        }
        if let Some(session) = &stamped {
            if session.reply_route().connector() != incoming.source.as_str()
                || incoming.channel.as_ref().map(|channel| channel.as_str())
                    != Some(session.reply_route().channel())
            {
                return Err(SessionError::Denied);
            }
            service.validate_context(session).await?;
        }
        let native = NativeRequest::from_envelope(&incoming)?;
        let keys = native.lookup_keys();
        let mut ordered = keys.clone();
        ordered.sort();
        ordered.dedup();
        let mut permits = Vec::with_capacity(ordered.len());
        for key in ordered {
            let gate = {
                let mut gates = self.request_gates.lock().unwrap();
                gates.retain(|_, gate| gate.strong_count() > 0);
                match gates.get(&key).and_then(Weak::upgrade) {
                    Some(gate) => gate,
                    None => {
                        let gate = Arc::new(AsyncMutex::new(()));
                        gates.insert(key, Arc::downgrade(&gate));
                        gate
                    }
                }
            };
            permits.push(gate.lock_owned().await);
        }
        let admission = for_message(&incoming, ctx)?;
        let (binding, lifecycle) = match &self.session_history {
            Some(history) => {
                let (bindings, lease) = history.find_requests_leased(&keys).await?;
                (native.reconcile(bindings)?, Some(lease))
            }
            None => (None, None),
        };
        let decision = NativeDecision {
            request: native,
            replay: binding.clone(),
        };
        let session = match (stamped, binding) {
            (Some(stamped), Some(binding))
                if stamped.session_id() != binding.journal.session_id() =>
            {
                return Err(SessionError::Denied)
            }
            (Some(stamped), _) => stamped,
            (None, Some(binding)) => {
                service
                    .context_for_message(admission, binding.journal.session_id())
                    .await?
            }
            (None, None) => service.admit(admission).await?,
        };
        service.refresh_presentation(&session).await?;
        let mut admitted = (*incoming).clone();
        admitted.context.insert(session);
        admitted.context.insert(decision);
        admitted.context.insert(RequestPermit {
            request: Mutex::new(Some(permits)),
            lifecycle: Mutex::new(lifecycle),
        });
        Ok(Arc::new(admitted))
    }
}
