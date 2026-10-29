//! Connect a UI-first installation to a subsequently configured owner transport.
//! Existing external identities are immutable: this never merges old users or
//! assigns other people's chats to the owner account.
use std::path::Path;

use sqlx::query_scalar;

use crate::{
    db::{
        error::DatabaseResult,
        repo::identity::{bind, find_in},
    },
    service::session::SessionService,
    web::configured_owner,
};

pub(super) async fn attach_configured_owner(
    service: &SessionService,
    manifest: &Path,
) -> DatabaseResult<()> {
    let Some((connector, actor)) = configured_owner(manifest) else {
        return Ok(());
    };
    let mut tx = service.database().pool().begin().await?;
    let user:Option<Option<String>>=query_scalar("SELECT canonical_user_id FROM albert_login_accounts WHERE id='bootstrap-owner' AND disabled=0 AND role='owner'")
        .fetch_optional(&mut *tx).await?;
    if let Some(Some(user)) = user {
        if find_in(&mut tx, &connector, &actor).await?.is_none() {
            bind(&mut tx, &connector, &actor, &user).await?;
        }
    }
    tx.commit().await?;
    Ok(())
}
