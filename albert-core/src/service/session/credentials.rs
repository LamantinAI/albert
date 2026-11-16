use octo_history::AppendRequest;
use sha2::{Digest, Sha256};

use super::SessionService;
use crate::{
    credentials::CREDENTIAL_RECEIPT_MARKER,
    history::{HistoryStore, Turn},
    sessions::error::SessionError,
};

impl SessionService {
    /// Manual entry is an explicit owner control action. Retain only its name
    /// for the next ordinary turn; never wake a model or export a temporary chat.
    pub async fn record_manual_credentials(&self) {
        for (receipt, context) in self.credentials().manual_receipts() {
            if let Err(error) = self.validate_context(&context).await {
                if matches!(
                    error,
                    SessionError::Denied
                        | SessionError::Ended
                        | SessionError::Expired
                        | SessionError::NotFound
                ) {
                    self.credentials().notification_recorded(&receipt.id, false);
                }
                continue;
            }
            if !context.authorized().await {
                self.credentials().notification_recorded(&receipt.id, false);
                continue;
            }
            let Some(name) = receipt.name.as_deref() else {
                continue;
            };
            let text=format!("{CREDENTIAL_RECEIPT_MARKER}Server credential {name} was saved by the owner. The value is withheld. Use the name for configuration; saving alone does not apply a connector.");
            let request = AppendRequest {
                namespace: "albert/credential-manual".into(),
                idempotency_key: receipt.id.clone(),
                digest: format!("{:x}", Sha256::digest(text.as_bytes())),
                turns: vec![Turn::user(text)],
            };
            if self
                .router
                .append_journal(&context.root_history_key(), &request)
                .await
                .is_ok()
            {
                self.credentials().notification_recorded(&receipt.id, true);
            }
        }
    }
}
