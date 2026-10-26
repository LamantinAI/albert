use octo_history::{CreateSession, LegacySessionImport, LegacySessionReceipt, SessionAccess};

use super::SessionService;
use crate::{
    db::error::DatabaseError,
    sessions::{
        error::{SessionError, SessionResult},
        validate_identifier, UserId, VerifiedLegacyImport,
    },
};

impl SessionService {
    /// The source adapter supplies observed inventories and verified bindings.
    /// A failed root or child assignment rolls back every catalog/default/remap.
    pub async fn import_verified_legacy(
        &self,
        imports: &[VerifiedLegacyImport],
    ) -> SessionResult<Vec<LegacySessionReceipt>> {
        let mut requests = Vec::with_capacity(imports.len());
        for import in imports {
            validate_identifier(&import.origin.connector)?;
            validate_identifier(&import.origin.channel)?;
            match &import.access {
                SessionAccess::Private { owner } => {
                    UserId::parse(owner)?;
                }
                SessionAccess::GroupBound { connector, channel } => {
                    if connector != &import.origin.connector || channel != &import.origin.channel {
                        return Err(SessionError::Denied);
                    }
                }
            }
            let mut request = CreateSession::private("");
            request.id = Some(import.session_id.clone());
            request.access = import.access.clone();
            requests.push(LegacySessionImport {
                source: import.source.clone(),
                origin: import.origin.clone(),
                session: request,
            });
        }
        let mut tx = self.history.begin_journal().await?;
        let receipts = self
            .history
            .import_legacy_sessions_in(&mut tx, &requests)
            .await?;
        for import in imports {
            for child in &import.children {
                self.history
                    .import_child_journal_in(
                        &mut tx,
                        &import.session_id,
                        &child.run_id,
                        &child.source,
                    )
                    .await?;
            }
        }
        tx.commit().await.map_err(DatabaseError::from)?;
        Ok(receipts)
    }
}
