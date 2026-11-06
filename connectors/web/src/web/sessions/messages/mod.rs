use albert_core::{
    history::preview_turn, history::project_session_record, history::PreviewContent, history::Turn,
    history::ACTION_MARKER, media::decode as decode_media, messaging::contract::MESSAGE_MARKER,
    service::session::web::WebActor,
};
use octo_history::{HistoryStore, PageRequest, Role};
use serde_json::from_str;

use super::{cursor::Cursor, media::asset_view, timestamp, SessionHost};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::MessageList,
    api::models::MessageView, api::models::PageQuery, api::models::SelectedSkillView,
};

const MAX_ACTIVITY_PAGES: usize = 8;

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
        let (mut before, mut through) = match query.after {
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
        let mut scanned = 0;
        let page = loop {
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
                    ApiError::conflict(
                        "Message page changed or chat ended. Reload before retrying.",
                    )
                })?;
            scanned += 1;
            through.get_or_insert(page.through_id);
            let readable = page
                .records
                .iter()
                .any(|record| display(record.turn.role, &record.turn.content).0 != "activity");
            if readable || !page.has_more || scanned >= MAX_ACTIVITY_PAGES {
                break page;
            }
            before = page.next_before;
        };
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
                let media = decode_media(&record.turn.content);
                let raw = media
                    .as_ref()
                    .map(|m| m.text.as_str())
                    .unwrap_or(&record.turn.content);
                let (role, content) = display(record.turn.role, raw);
                MessageView {
                    skills: media
                        .as_ref()
                        .map(|media| {
                            media
                                .skills
                                .iter()
                                .map(|skill| SelectedSkillView {
                                    id: skill.id.clone(),
                                    name: skill.name.clone(),
                                    revision: skill.revision.clone(),
                                })
                                .collect()
                        })
                        .unwrap_or_default(),
                    attachments: media
                        .map(|m| m.attachments.into_iter().map(asset_view).collect())
                        .unwrap_or_default(),
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
    if content.starts_with(MESSAGE_MARKER) {
        return match preview_turn(&Turn::assistant(content.to_owned())) {
            PreviewContent::Text(text) => ("assistant", text),
            PreviewContent::Service => (
                "activity",
                project_session_record(content)
                    .filter(|text| !text.is_empty())
                    .unwrap_or_else(|| "Session metadata recorded.".into()),
            ),
            PreviewContent::MediaOnly => ("activity", "Media recorded.".into()),
            PreviewContent::Unreadable => (
                "activity",
                "Stored session activity could not be read.".into(),
            ),
        };
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

#[cfg(test)]
mod tests {
    use albert_core::{history::assistant_turn, messaging::contract::MESSAGE_MARKER};
    use octo_history::{AppendRequest, HistoryStore, PageRequest, Role, Turn};
    use serde_json::json;

    use super::{super::fixture::fixture, super::SessionHost, display};
    use crate::api::models::PageQuery;

    async fn append(host: &SessionHost, journal: &str, key: &str, turns: Vec<Turn>) {
        host.runtime
            .sessions
            .journal_router()
            .append_journal(
                journal,
                &AppendRequest {
                    namespace: "activity-pages".into(),
                    idempotency_key: key.into(),
                    digest: key.into(),
                    turns,
                },
            )
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn service_page_scan_is_bounded_and_continues_the_original_snapshot() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Paged", false)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let journal = context.root_history_key();
        let mut turns = vec![Turn::user("Readable history")];
        turns.extend((0..20).map(|_| Turn::assistant("[albert tool journal v2]\n[]")));
        append(&host, &journal, "initial", turns).await;
        let mut page = host
            .messages(
                &actor,
                session.id.as_str(),
                PageQuery {
                    limit: Some(1),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert!(page.items.iter().all(|item| item.role == "activity"));
        assert!(page.has_more);
        let watermark = page.through_id.clone();
        append(&host, &journal, "later", vec![Turn::user("Later append")]).await;
        let mut found = false;
        for _ in 0..4 {
            page = host
                .messages(
                    &actor,
                    session.id.as_str(),
                    PageQuery {
                        limit: Some(1),
                        after: page.next_cursor.clone(),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            assert_eq!(page.through_id, watermark);
            assert!(page.items.iter().all(|item| item.content != "Later append"));
            if page
                .items
                .iter()
                .any(|item| item.content == "Readable history")
            {
                found = true;
                break;
            }
            assert!(page.has_more);
        }
        assert!(found);
        assert!(!page.has_more);
        let stored = host
            .runtime
            .sessions
            .journal_router()
            .journal_page(
                &journal,
                PageRequest {
                    limit: 100,
                    before_id: None,
                    through_id: None,
                },
            )
            .await
            .unwrap();
        assert_eq!(stored.records.len(), 22);
    }

    #[tokio::test]
    async fn mixed_pages_are_not_skipped_and_service_only_history_exhausts() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        for mixed in [false, true] {
            let session = host
                .runtime
                .sessions
                .web_create(&actor, "History", false)
                .await
                .unwrap();
            let context = host.resource(&actor, session.id.as_str()).await.unwrap();
            let journal = context.root_history_key();
            let mut turns = vec![Turn::assistant("[albert tool journal v2]\n[]"); 3];
            if mixed {
                turns.push(Turn::user("Visible message"));
            }
            turns.push(Turn::assistant("[albert tool journal v2]\n[]"));
            append(&host, &journal, "initial", turns).await;
            let page = host
                .messages(
                    &actor,
                    session.id.as_str(),
                    PageQuery {
                        limit: Some(2),
                        ..Default::default()
                    },
                )
                .await
                .unwrap();
            if mixed {
                assert_eq!(page.items.len(), 2);
                assert!(page
                    .items
                    .iter()
                    .any(|item| item.content == "Visible message"));
                assert!(page.has_more);
            } else {
                assert!(page.items.iter().all(|item| item.role == "activity"));
                assert!(!page.has_more);
                assert!(page.next_cursor.is_none());
            }
        }
    }

    #[test]
    fn linked_results_and_control_audits_never_dump_internal_tuples_or_authority() {
        let result = format!(
            "{MESSAGE_MARKER}{}",
            json!(["albert.session_work.result.v1","message",{"owner":"not-user-input"},{"ceiling":"internal"},"Readable linked answer",null])
        );
        let (role, text) = display(Role::Assistant, &result);
        assert_eq!(role, "assistant");
        assert!(text.contains("Readable linked answer"));
        assert!(
            !text.contains("ceiling")
                && !text.contains("not-user-input")
                && !text.contains(MESSAGE_MARKER)
        );
        let rename = format!(
            "{MESSAGE_MARKER}{}",
            json!([
                "albert.session_title.renamed.v1",
                "session",
                "Literal title"
            ])
        );
        assert_eq!(
            display(Role::Assistant, &rename),
            ("activity", "Session metadata recorded.".into())
        );
        let failure = format!(
            "{MESSAGE_MARKER}{}",
            json!(["albert.work_report.failed.v1", "work", "revoked"])
        );
        let (role, text) = display(Role::Assistant, &failure);
        assert_eq!(role, "activity");
        assert!(text.contains("delivery failed: revoked"));
        assert!(!text.contains("albert.work_report"));
    }
    #[test]
    fn escaped_assistant_literals_and_existing_tool_activity_keep_their_meaning() {
        let literal = format!(
            "{MESSAGE_MARKER}{}",
            json!(["albert.session_title.renamed.v1", "quoted", "ordinary text"])
        );
        let stored = assistant_turn(literal.clone());
        assert_eq!(
            display(Role::Assistant, &stored.content),
            ("assistant", literal)
        );
        assert_eq!(
            display(Role::Assistant, "[albert tool journal v2]\n[]"),
            ("activity", "Tool activity recorded.".into())
        );
    }
}
