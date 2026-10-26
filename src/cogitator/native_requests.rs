//! Native aliases are host transport identities. They are never reconstructed
//! from rendered message text, last-frame tags or model-provided owner fields.
use std::collections::HashSet;

use octo_connector_telegram::NativeBatch;
use octo_core::Envelope;
use octo_history::{AppendRequest, RequestAlias, RequestBinding, Turn};
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use crate::{
    acl::tag,
    sessions::{
        error::{SessionError, SessionResult},
        native::{batch_identity, request_alias},
    },
};

#[derive(Clone)]
pub(super) struct NativeRequest {
    pub namespace: String,
    pub key: String,
    pub digest: Option<String>,
    pub aliases: Vec<RequestAlias>,
}

#[derive(Clone)]
pub(super) struct NativeDecision {
    pub request: NativeRequest,
    pub replay: Option<RequestBinding>,
}

impl NativeRequest {
    pub fn from_envelope(incoming: &Envelope) -> SessionResult<Self> {
        if let Some(batch) = incoming.context.get::<NativeBatch>() {
            if batch.frames.is_empty() {
                return Err(SessionError::InvalidInput("empty native batch"));
            }
            let mut aliases = Vec::with_capacity(batch.frames.len());
            let mut unique = HashSet::new();
            for frame in &batch.frames {
                if frame.connector != incoming.source
                    || incoming
                        .channel
                        .as_ref()
                        .map(|channel| channel.as_str().to_owned())
                        != Some(frame.chat_id.to_string())
                {
                    return Err(SessionError::Denied);
                }
                let alias = request_alias(frame);
                if !unique.insert((alias.namespace.clone(), alias.idempotency_key.clone())) {
                    return Err(SessionError::InvalidInput(
                        "duplicate constituent in native batch",
                    ));
                }
                aliases.push(alias);
            }
            let (namespace, key, digest) = batch_identity(&aliases)?;
            return Ok(Self {
                namespace,
                key,
                digest: Some(digest),
                aliases,
            });
        }
        let origin = to_vec(&(
            incoming.source.as_str(),
            incoming.channel.as_ref().map(|channel| channel.as_str()),
        ))
        .expect("source strings serialize");
        Ok(Self {
            namespace: format!("albert/native/{:x}", Sha256::digest(origin)),
            key: tag(incoming, "message_id")
                .map(str::to_owned)
                .unwrap_or_else(|| incoming.id.to_string()),
            digest: None,
            aliases: Vec::new(),
        })
    }

    pub fn lookup_keys(&self) -> Vec<(String, String)> {
        if self.aliases.is_empty() {
            vec![(self.namespace.clone(), self.key.clone())]
        } else {
            self.aliases
                .iter()
                .map(|alias| (alias.namespace.clone(), alias.idempotency_key.clone()))
                .collect()
        }
    }

    pub fn reconcile(
        &self,
        bindings: Vec<Option<RequestBinding>>,
    ) -> SessionResult<Option<RequestBinding>> {
        if bindings.len() != self.lookup_keys().len() {
            return Err(SessionError::InvalidInput("incomplete native lookup"));
        }
        if self.aliases.is_empty() {
            return Ok(bindings.into_iter().next().flatten());
        }
        if bindings.iter().all(Option::is_none) {
            return Ok(None);
        }
        if bindings.iter().any(Option::is_none) {
            return Err(SessionError::InvalidInput(
                "partially replayed native batch; no work started",
            ));
        }
        let mut found = None;
        for (alias, binding) in self.aliases.iter().zip(bindings.into_iter()) {
            let binding = binding.ok_or(SessionError::InvalidInput("missing native receipt"))?;
            if binding.digest != alias.digest {
                return Err(SessionError::InvalidInput(
                    "native content conflicts with accepted receipt",
                ));
            }
            if let Some(first) = &found {
                let first: &RequestBinding = first;
                if first.journal != binding.journal
                    || first.batch_digest != binding.batch_digest
                    || first.receipt.record_ids != binding.receipt.record_ids
                    || first.receipt.through_id != binding.receipt.through_id
                {
                    return Err(SessionError::InvalidInput(
                        "native constituents belong to different accepted batches",
                    ));
                }
            } else {
                found = Some(binding);
            }
        }
        Ok(found)
    }

    pub fn append_request(&self, digest: String, turns: Vec<Turn>) -> AppendRequest {
        AppendRequest {
            namespace: self.namespace.clone(),
            idempotency_key: self.key.clone(),
            digest: self.digest.clone().unwrap_or(digest),
            turns,
        }
    }
}
