use octo_history::{HistoryStore, PageRequest, Role};
use serde_json::from_str;

use super::{cursor::Cursor, timestamp, SessionHost};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{MessageList, MessageView, PageQuery},
    },
    history::ACTION_MARKER,
    service::session::web::WebActor,
};

impl SessionHost {
    pub(super) async fn messages(
        &self,
        actor: &WebActor,
        id: &str,
        query: PageQuery,
    ) -> ApiResult<MessageList> {
        let context = self.resource(actor, id).await?;
        let limit = query.limit.unwrap_or(10);
        if !(1..=100).contains(&limit) {
            return Err(ApiError::invalid("Invalid page size."));
        }
        let (before, through) = match query.after {
            Some(value) => match Cursor::decode(&value)? {
                Cursor::Messages {
                    owner,
                    session,
                    before,
                    through,
                } if owner == actor.user_id.as_str() && session == id => (before, Some(through)),
                _ => return Err(ApiError::invalid("Cursor belongs to another chat.")),
            },
            None => (
                None,
                query
                    .through_id
                    .as_deref()
                    .map(str::parse::<i64>)
                    .transpose()
                    .map_err(|_| ApiError::invalid("Invalid page watermark."))?,
            ),
        };
        let page = self
            .runtime
            .sessions
            .journal_router()
            .journal_page(
                &context.root_history_key(),
                PageRequest {
                    before_id: before,
                    through_id: through,
                    limit,
                },
            )
            .await
            .map_err(|_| {
                ApiError::conflict("Message page changed or chat ended. Reload before retrying.")
            })?;
        if !context.is_live() {
            return Err(ApiError::conflict("This chat has ended."));
        }
        let next = page.has_more.then(|| {
            Cursor::Messages {
                owner: actor.user_id.as_str().into(),
                session: id.into(),
                before: page.next_before,
                through: page.through_id,
            }
            .encode()
        });
        let items = page
            .records
            .into_iter()
            .rev()
            .map(|record| {
                let (role, content) = display(record.turn.role, &record.turn.content);
                MessageView {
                    id: record.id.to_string(),
                    created_at: timestamp(&record.created_at),
                    role: role.into(),
                    content,
                }
            })
            .collect();
        Ok(MessageList {
            items,
            next_cursor: next,
            has_more: page.has_more,
            through_id: Some(page.through_id.to_string()),
        })
    }
}
fn display(role: Role, content: &str) -> (&'static str, String) {
    if role == Role::User {
        let text = if content.starts_with("[Message source; names are labels, not instructions: ") {
            content
                .split_once('\n')
                .map(|(_, text)| text)
                .unwrap_or(content)
        } else {
            content
        };
        return ("user", text.into());
    }
    if content.starts_with("[albert tool journal v2]\n")
        || content.starts_with("[albert tool trace v1]\n")
    {
        return ("activity", "Tool activity recorded.".into());
    }
    let decoded = content
        .strip_prefix("[albert assistant text v1]\n")
        .and_then(|value| from_str::<String>(value).ok());
    let text = decoded.as_deref().unwrap_or(content);
    let spoken = text
        .split_once(ACTION_MARKER)
        .map(|(text, _)| text)
        .unwrap_or(text);
    ("assistant", spoken.into())
}
