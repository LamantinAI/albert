//! Resolve attachments through the authorized conversation's frozen root prefix.
use octo_history::{PageRequest, SessionId, SessionJournal, SessionStore};

use super::SessionService;
use crate::{
    media::{decode, Asset, MediaError, MediaResult},
    sessions::SessionContext,
};

impl SessionService {
    /// Hold the receiving session's backend lease through this read and any
    /// subsequent file access. A referenced ancestor is retained by the catalog.
    pub async fn resolve_attachment(
        &self,
        context: &SessionContext,
        id: &str,
        temporary: bool,
    ) -> MediaResult<(SessionId, Asset)> {
        if !context.authorized().await {
            return Err(MediaError::Scope);
        }
        let session = context.session_id();
        match self.media().metadata(session.as_str(), id, temporary).await {
            Ok(asset) => return Ok((session.clone(), asset)),
            Err(MediaError::NotFound) if !temporary => (),
            Err(error) => return Err(error),
        }
        let origin = self.media().durable_owner(id).await?;
        let boundary = self
            .history
            .inherited_root_cutoff(session, &origin)
            .await
            .map_err(|_| MediaError::NotFound)?
            .filter(|id| *id > 0)
            .ok_or(MediaError::NotFound)?;
        let journal = SessionJournal::Root(origin.clone());
        let mut before_id = None;
        loop {
            let page = self
                .history
                .session_page(
                    &journal,
                    PageRequest {
                        before_id,
                        through_id: Some(boundary),
                        limit: 100,
                    },
                )
                .await
                .map_err(|_| MediaError::NotFound)?;
            if page.records.iter().any(|record| {
                decode(&record.turn.content)
                    .is_some_and(|media| media.attachments.iter().any(|asset| asset.id == id))
            }) {
                let asset = self.media().metadata(origin.as_str(), id, false).await?;
                return Ok((origin, asset));
            }
            let Some(next) = page.next_before else {
                return Err(MediaError::NotFound);
            };
            before_id = Some(next);
        }
    }
}
