use octo_history::{CreateSession, Session, SessionOrigin};
use sqlx::{query_as, Sqlite, Transaction};

use crate::{
    db::entity::PrivateRouteKey, db::error::DatabaseError, db::repo::route::set,
    service::session::SessionService, sessions::error::SessionError,
    sessions::error::SessionResult,
};

impl SessionService {
    pub(super) async fn rotate_deleted_selectors(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        deleted: &Session,
        owner: &str,
    ) -> SessionResult<()> {
        for origin in self.history.default_origins_in(tx, &deleted.id).await? {
            let replacement = self.fresh_standard(tx, owner, &deleted.host).await?;
            self.history
                .replace_default_session_in(tx, &origin, &deleted.id, &replacement)
                .await?;
        }
        let routes: Vec<(String,String,String,Option<String>,i64)> = query_as("SELECT connector_id,channel_id,session_id,return_session_id,revision FROM albert_private_session_routes WHERE user_id=? AND (session_id=? OR return_session_id=?)")
            .bind(owner).bind(deleted.id.as_str()).bind(deleted.id.as_str()).fetch_all(&mut **tx).await.map_err(DatabaseError::from)?;
        for (connector, channel, selected, back, revision) in routes {
            let origin = SessionOrigin {
                connector: connector.clone(),
                channel: channel.clone(),
            };
            let default = match self.history.get_default_session_in(tx, &origin).await? {
                Some(default) => default,
                None => {
                    let replacement = self.fresh_standard(tx, owner, &deleted.host).await?;
                    self.history
                        .ensure_default_session_in(
                            tx,
                            &origin,
                            CreateSession {
                                id: Some(replacement.id.clone()),
                                access: replacement.access.clone(),
                                host: replacement.host.clone(),
                                title: replacement.title.clone(),
                                kind: replacement.kind,
                            },
                        )
                        .await?
                }
            };
            if default.access != deleted.access {
                return Err(SessionError::Denied);
            }
            let key = PrivateRouteKey {
                connector_id: connector,
                channel_id: channel,
                user_id: owner.into(),
            };
            let current = if selected == deleted.id.as_str() {
                default.id.as_str()
            } else {
                &selected
            };
            let back = back.as_deref().map(|id| {
                if id == deleted.id.as_str() {
                    default.id.as_str()
                } else {
                    id
                }
            });
            set(tx, &key, current, back, Some(revision)).await?;
        }
        Ok(())
    }
    async fn fresh_standard(
        &self,
        tx: &mut Transaction<'_, Sqlite>,
        owner: &str,
        host: &str,
    ) -> SessionResult<Session> {
        let mut create = CreateSession::private(owner);
        create.title = "Standard".into();
        create.host = host.into();
        Ok(self.history.create_session_in(tx, create).await?)
    }
}
