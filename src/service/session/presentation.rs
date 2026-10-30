//! Refresh a synchronous visibility cache from an authoritative current route.
use octo_history::SessionId;

use super::{route_key, SessionService};
use crate::{
    db::repo::route::find,
    sessions::{error::SessionResult, SessionContext},
};

impl SessionService {
    pub async fn refresh_presentation(&self, context: &SessionContext) -> SessionResult<bool> {
        if !context.is_private() {
            return Ok(true);
        }
        let key = route_key(context)?;
        let Some(route) = find(self.database.pool(), &key).await? else {
            return Ok(false);
        };
        self.presentations
            .observe(&key, &SessionId::parse(&route.session_id)?, route.revision);
        Ok(true)
    }
}
