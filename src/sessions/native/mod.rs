//! One canonical identity shared by pre-IO Telegram policy and runtime receipt
//! admission. Provider dates distinguish update IDs reused after an idle reset.

use octo_connector_telegram::NativeFrame;
use octo_history::RequestAlias;
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use super::error::{SessionError, SessionResult};

pub(crate) fn request_alias(frame: &NativeFrame) -> RequestAlias {
    let issuer = to_vec(&(frame.bot_id, frame.connector.as_str(), frame.chat_id))
        .expect("native identity serializes");
    RequestAlias {
        namespace: format!("telegram.native.{:x}", Sha256::digest(issuer)),
        idempotency_key: format!(
            "update:{}:message:{}:at:{}",
            frame.update_id, frame.message_id, frame.date
        ),
        digest: frame.digest.clone(),
    }
}

/// A batch identity depends on its full constituent set, not its last message.
/// Digests bind semantics independently of request IDs. The body still retains
/// provider order; retries with the same constituents are already accepted.
pub(crate) fn batch_identity(aliases: &[RequestAlias]) -> SessionResult<(String, String, String)> {
    let first = aliases
        .first()
        .ok_or(SessionError::InvalidInput("empty native batch"))?;
    if aliases
        .iter()
        .any(|alias| alias.namespace != first.namespace)
    {
        return Err(SessionError::InvalidInput(
            "native batch crosses transport identities",
        ));
    }
    if aliases.len() == 1 {
        return Ok((
            first.namespace.clone(),
            first.idempotency_key.clone(),
            first.digest.clone(),
        ));
    }
    let mut parts: Vec<_> = aliases
        .iter()
        .map(|alias| {
            (
                alias.namespace.as_str(),
                alias.idempotency_key.as_str(),
                alias.digest.as_str(),
            )
        })
        .collect();
    parts.sort_unstable();
    if parts.windows(2).any(|pair| pair[0].1 == pair[1].1) {
        return Err(SessionError::InvalidInput(
            "duplicate native batch constituent",
        ));
    }
    let ids: Vec<_> = parts.iter().map(|part| (part.0, part.1)).collect();
    let key = format!(
        "batch/{:x}",
        Sha256::digest(to_vec(&ids).expect("native IDs serialize"))
    );
    let digest = format!(
        "{:x}",
        Sha256::digest(to_vec(&parts).expect("native hashes serialize"))
    );
    Ok((format!("{}.batch", first.namespace), key, digest))
}

#[cfg(test)]
mod tests {
    use octo_connector_telegram::NativeFrame;
    use octo_core::ConnectorId;

    use super::{batch_identity, request_alias};

    fn frame(bot_id: u64, date: i64) -> NativeFrame {
        NativeFrame {
            bot_id,
            connector: ConnectorId::new("telegram"),
            update_id: 9,
            message_id: 7,
            date,
            chat_id: 7,
            actor_id: Some(7),
            chat_type: "private".into(),
            command_text: None,
            digest: "semantic-hash".into(),
            media: false,
        }
    }

    #[test]
    fn reset_update_id_and_another_bot_cannot_alias_an_old_message() {
        let first = request_alias(&frame(1, 1_000_000));
        let later = request_alias(&frame(1, 2_000_000));
        let another = request_alias(&frame(2, 1_000_000));
        assert_ne!(first.idempotency_key, later.idempotency_key);
        assert_ne!(first.namespace, another.namespace);
    }

    #[test]
    fn batch_uses_every_constituent_and_order_independent_retry_identity() {
        let one = request_alias(&frame(1, 1_000_000));
        let two = request_alias(&frame(1, 1_000_001));
        let left = batch_identity(&[one.clone(), two.clone()]).unwrap();
        let right = batch_identity(&[two, one.clone()]).unwrap();
        assert_eq!(left, right);
        assert_ne!(left.1, one.idempotency_key);
        assert!(batch_identity(&[one.clone(), one]).is_err());
    }
}
