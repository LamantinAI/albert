//! One canonical identity shared by pre-IO Telegram policy and runtime receipt
//! admission. Provider dates distinguish update IDs reused after an idle reset.

use octo_history::RequestAlias;
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use super::{error::SessionError, error::SessionResult};

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
    use octo_history::RequestAlias;

    use super::batch_identity;

    fn alias(date: i64) -> RequestAlias {
        RequestAlias {
            namespace: "native.test".into(),
            idempotency_key: format!("request/{date}"),
            digest: "semantic-hash".into(),
        }
    }

    #[test]
    fn batch_uses_every_constituent_and_order_independent_retry_identity() {
        let one = alias(1_000_000);
        let two = alias(1_000_001);
        let left = batch_identity(&[one.clone(), two.clone()]).unwrap();
        let right = batch_identity(&[two, one.clone()]).unwrap();
        assert_eq!(left, right);
        assert_ne!(left.1, one.idempotency_key);
        assert!(batch_identity(&[one.clone(), one]).is_err());
    }
}
