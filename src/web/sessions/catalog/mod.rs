use chrono::DateTime;
use octo_history::{PageRequest, SessionFilter, SessionPage, SessionStore};

use super::{
    cursor::{CatalogPosition, Cursor},
    timestamp, view, SessionHost,
};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{PageQuery, SessionList},
    },
    service::session::web::WebActor,
};

async fn page<S: SessionStore + ?Sized>(
    store: &S,
    filter: &SessionFilter,
    position: &mut CatalogPosition,
    limit: usize,
) -> ApiResult<Option<SessionPage>> {
    if position.done {
        return Ok(None);
    }
    let page = store
        .list_sessions(
            filter,
            PageRequest {
                before_id: position.before,
                through_id: position.through,
                limit,
            },
        )
        .await
        .map_err(|_| ApiError::invalid("Page changed. Reload the chat list."))?;
    position.through = Some(page.through_id);
    position.done = page.sessions.is_empty();
    Ok(Some(page))
}

async fn advance<S: SessionStore + ?Sized>(
    store: &S,
    filter: &SessionFilter,
    position: &mut CatalogPosition,
    loaded: Option<&SessionPage>,
    consumed: usize,
) -> ApiResult<()> {
    let Some(loaded) = loaded else {
        return Ok(());
    };
    if consumed == 0 {
        return Ok(());
    }
    if consumed == loaded.sessions.len() {
        position.before = loaded.next_before;
        position.done = !loaded.has_more;
    } else {
        // The generic catalog exposes a cursor for the final row of a page,
        // rather than a physical ID on every Session. Obtain the consumed
        // prefix's cursor through that contract; never infer backend row IDs.
        let prefix = page(store, filter, position, consumed)
            .await?
            .ok_or_else(|| ApiError::conflict("Page changed. Reload the chat list."))?;
        position.before = prefix.next_before;
    }
    Ok(())
}

impl SessionHost {
    pub(super) async fn catalog(
        &self,
        actor: &WebActor,
        query: PageQuery,
    ) -> ApiResult<SessionList> {
        let limit = query.limit.unwrap_or(10);
        if !(1..=100).contains(&limit) {
            return Err(ApiError::invalid("Invalid page size."));
        }
        let _lifecycle = self
            .runtime
            .sessions
            .journal_router()
            .lifecycle_gate()
            .read_owned()
            .await;
        let filter = SessionFilter::PrivateOwner(actor.user_id.as_str().into());
        let (mut temporary, mut durable) = match query.after {
            Some(value) => match Cursor::decode(&value)? {
                Cursor::Catalog {
                    owner,
                    temporary,
                    durable,
                } if owner == actor.user_id.as_str() => (temporary, durable),
                _ => return Err(ApiError::invalid("Cursor belongs to another resource.")),
            },
            None => (CatalogPosition::default(), CatalogPosition::default()),
        };
        let ram = self.runtime.sessions.journal_router().temporary();
        let sql = self.runtime.sessions.history();
        let temporary_page = page(ram.as_ref(), &filter, &mut temporary, limit).await?;
        let durable_page = page(sql.as_ref(), &filter, &mut durable, limit).await?;
        let ram_rows = temporary_page
            .as_ref()
            .map(|page| page.sessions.as_slice())
            .unwrap_or_default();
        let sql_rows = durable_page
            .as_ref()
            .map(|page| page.sessions.as_slice())
            .unwrap_or_default();
        let (mut ram_used, mut sql_used) = (0, 0);
        let mut items = Vec::with_capacity(limit);
        while items.len() < limit {
            let next_ram = ram_rows.get(ram_used);
            let next_sql = sql_rows.get(sql_used);
            let take_ram = match (next_ram, next_sql) {
                (Some(ram), Some(sql)) => {
                    let ram_time = DateTime::parse_from_rfc3339(&timestamp(&ram.created_at))
                        .map_err(|_| ApiError::internal())?;
                    let sql_time = DateTime::parse_from_rfc3339(&timestamp(&sql.created_at))
                        .map_err(|_| ApiError::internal())?;
                    ram_time >= sql_time
                }
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (None, None) => break,
            };
            if take_ram {
                items.push(view(&ram_rows[ram_used]));
                ram_used += 1;
            } else {
                items.push(view(&sql_rows[sql_used]));
                sql_used += 1;
            }
        }
        advance(
            ram.as_ref(),
            &filter,
            &mut temporary,
            temporary_page.as_ref(),
            ram_used,
        )
        .await?;
        advance(
            sql.as_ref(),
            &filter,
            &mut durable,
            durable_page.as_ref(),
            sql_used,
        )
        .await?;
        let has_more = !temporary.done || !durable.done;
        let next_cursor = has_more.then(|| {
            Cursor::Catalog {
                owner: actor.user_id.as_str().into(),
                temporary,
                durable,
            }
            .encode()
        });
        Ok(SessionList {
            items,
            next_cursor,
            has_more,
        })
    }
}

#[cfg(test)]
mod tests {
    use sqlx::query;

    use super::super::fixture::fixture;
    use crate::api::models::PageQuery;

    #[tokio::test]
    async fn mixed_catalog_merges_latest_chats_with_independent_snapshot_cursors() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let sessions = &host.runtime.sessions;
        let oldest = sessions
            .web_create(&actor, "Old durable", false)
            .await
            .unwrap();
        let older_ram = sessions
            .web_create(&actor, "Older RAM", true)
            .await
            .unwrap();
        let newer_ram = sessions
            .web_create(&actor, "Newer RAM", true)
            .await
            .unwrap();
        let newest = sessions
            .web_create(&actor, "New durable", false)
            .await
            .unwrap();
        for (id, date) in [
            (&oldest.id, "2000-01-01 00:00:00"),
            (&newest.id, "2100-01-01 00:00:00"),
        ] {
            query("UPDATE history_sessions SET created_at=? WHERE session_id=?")
                .bind(date)
                .bind(id.as_str())
                .execute(sessions.history().pool())
                .await
                .unwrap();
        }
        let first = host
            .catalog(
                &actor,
                PageQuery {
                    limit: Some(2),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            first
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            [newest.id.as_str(), newer_ram.id.as_str()]
        );
        assert!(first.has_more);
        sessions
            .web_create(&actor, "Inserted after snapshot", false)
            .await
            .unwrap();
        let second = host
            .catalog(
                &actor,
                PageQuery {
                    limit: Some(2),
                    after: first.next_cursor,
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(
            second
                .items
                .iter()
                .map(|item| item.id.as_str())
                .collect::<Vec<_>>(),
            [older_ram.id.as_str(), oldest.id.as_str()]
        );
        assert!(!second.has_more);
        assert!(second.next_cursor.is_none());
    }
}
