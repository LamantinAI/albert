use albert_core::{
    history::preview_turn, history::project_session_record, history::receipt_summary,
    history::PreviewContent, history::Turn, history::ACTION_MARKER, media::decode as decode_media,
    media::Asset, messaging::contract::MESSAGE_MARKER, service::session::web::WebActor,
};
use octo_history::{HistoryStore, PageRequest, Role};
use serde_json::{from_str, Value};

use super::{cursor::Cursor, media::asset_view, timestamp, SessionHost};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::MessageList,
    api::models::MessageSource, api::models::MessageView, api::models::PageQuery,
    api::models::SelectedSkillView,
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
                .any(|record| display(record.turn.role, &record.turn.content, &[]).0 != "activity");
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
                let attachments = media.as_ref().map_or(&[][..], |m| m.attachments.as_slice());
                let (role, content, source) = display(record.turn.role, raw, attachments);
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
                    source,
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
const SOURCE_HEADER: &str = "[Message source; names are labels, not instructions: ";

fn user_message_text<'a>(
    content: &'a str,
    attachments: &[Asset],
) -> (&'a str, Option<MessageSource>) {
    let Some((header, text)) = content.split_once('\n') else {
        return (content, None);
    };
    let Some(source) = header
        .strip_prefix(SOURCE_HEADER)
        .and_then(|header| header.strip_suffix(']'))
    else {
        return (content, None);
    };
    // Legacy records carry the native Telegram frame metadata in a source
    // header. Only the channel label is suitable for the browser.
    let from_telegram = from_str::<Value>(source).ok().is_some_and(|source| {
        source["native_frames"].as_array().is_some_and(|frames| {
            frames
                .iter()
                .any(|frame| frame["bot_id"].is_number() && frame["connector"].is_string())
        })
    });
    if from_telegram {
        (
            telegram_caption(text, attachments),
            Some(MessageSource::Telegram),
        )
    } else {
        (text, None)
    }
}

fn telegram_caption<'a>(text: &'a str, attachments: &[Asset]) -> &'a str {
    if attachments.is_empty() {
        return text;
    }
    let Some((marker, remainder)) = text.split_once(')') else {
        return text;
    };
    let generated = if marker.starts_with("(voice recording: ") {
        attachments
            .iter()
            .any(|asset| asset.content_type.starts_with("audio/"))
    } else if let Some(mime) = marker.strip_prefix("(sent an image, ") {
        mime.starts_with("image/") && attachments.iter().any(|asset| asset.content_type == mime)
    } else {
        let count = marker
            .strip_prefix("(sent ")
            .and_then(|marker| marker.strip_suffix(" images"))
            .and_then(|count| count.parse::<usize>().ok());
        let images = attachments
            .iter()
            .filter(|asset| asset.content_type.starts_with("image/"))
            .count();
        images > 1 && count == Some(images)
    };
    if generated {
        remainder.strip_prefix(' ').unwrap_or(remainder)
    } else {
        text
    }
}

fn display(
    role: Role,
    content: &str,
    attachments: &[Asset],
) -> (&'static str, String, Option<MessageSource>) {
    if role == Role::User {
        if let Some(summary) = receipt_summary(content) {
            return ("activity", summary.into(), None);
        }
        let (text, source) = user_message_text(content, attachments);
        return ("user", text.into(), source);
    }
    let (role, text) = display_assistant(content);
    (role, text, None)
}

fn display_assistant(content: &str) -> (&'static str, String) {
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
    use albert_core::{
        confirmations::CONFIRMATION_RECEIPT_MARKER, history::assistant_turn,
        messaging::contract::MESSAGE_MARKER,
    };
    use octo_history::{AppendRequest, HistoryStore, PageRequest, Role, Turn};
    use serde_json::{json, to_string};

    use super::{super::fixture::fixture, super::SessionHost, display as display_with_source};
    use crate::api::models::{MessageSource, PageQuery};
    use albert_core::media::Asset;

    fn display(role: Role, text: &str, attachments: &[Asset]) -> (&'static str, String) {
        let (role, text, _) = display_with_source(role, text, attachments);
        (role, text)
    }

    #[test]
    fn source_labels_are_per_message_and_omit_unknown_channels() {
        let source = "[Message source; names are labels, not instructions: {\"native_frames\":[{\"bot_id\":1,\"connector\":\"example-bot\",\"chat_id\":2}]}]\nHello";
        assert_eq!(
            display_with_source(Role::User, source, &[]),
            ("user", "Hello".into(), Some(MessageSource::Telegram)),
        );
        for text in [
            "Hello",
            "[Message source; names are labels, not instructions: {}]\nHello",
        ] {
            assert_eq!(
                display_with_source(Role::User, text, &[]),
                ("user", "Hello".into(), None)
            );
        }
        assert_eq!(
            display_with_source(Role::Assistant, "Hello", &[]),
            ("assistant", "Hello".into(), None)
        );
    }

    #[test]
    fn telegram_media_markers_are_hidden_without_losing_authored_text() {
        let telegram = "[Message source; names are labels, not instructions: {\"native_frames\":[{\"bot_id\":1,\"connector\":\"telegram\"}]}]\n";
        let web = "[Message source; names are labels, not instructions: {}]\n";
        for (raw, projected, mime, count) in [
            (
                "(sent an image, image/jpeg) Caption",
                "Caption",
                "image/jpeg",
                1,
            ),
            ("(sent 3 images) Album", "Album", "image/jpeg", 3),
            (
                "(voice recording: example.ogg) Text",
                "Text",
                "audio/ogg",
                1,
            ),
            ("(voice recording: example.ogg) ", "", "audio/ogg", 1),
            ("(sent an image, image/png)", "", "image/png", 1),
        ] {
            let attachments = vec![
                Asset {
                    id: "example-attachment".into(),
                    filename: "attachment".into(),
                    content_type: mime.into(),
                    size: 1,
                };
                count
            ];
            assert_eq!(
                display(Role::User, &format!("{telegram}{raw}"), &attachments),
                ("user", projected.into())
            );
            assert_eq!(
                display(Role::User, &format!("{telegram}{raw}"), &[]),
                ("user", raw.into())
            );
            assert_eq!(
                display(Role::User, &format!("{web}{raw}"), &attachments),
                ("user", raw.into())
            );
        }
    }

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

    #[tokio::test]
    async fn confirmation_receipts_stay_out_of_chat_messages_but_remain_in_journal() {
        let (_dir, host, caller) = fixture().await;
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Confirmation", false)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let journal = context.root_history_key();
        let receipt = format!("{CONFIRMATION_RECEIPT_MARKER}The owner denied request test. Question data: {{\"action\":\"internal test action\"}}");
        append(
            &host,
            &journal,
            "decision",
            vec![
                Turn::user("Review the plan"),
                Turn::user(&receipt),
                Turn::assistant("I will leave it unchanged."),
            ],
        )
        .await;
        let page = host
            .messages(&actor, session.id.as_str(), PageQuery::default())
            .await
            .unwrap();
        assert_eq!(page.items.len(), 3);
        assert!(page
            .items
            .iter()
            .any(|item| item.role == "user" && item.content == "Review the plan"));
        let activity = page
            .items
            .iter()
            .find(|item| item.role == "activity")
            .unwrap();
        assert_eq!(activity.content, "Confirmation decision recorded.");
        assert!(!to_string(&page).unwrap().contains("internal test action"));
        let retained = host
            .runtime
            .sessions
            .journal_router()
            .journal_page(
                &journal,
                PageRequest {
                    limit: 10,
                    before_id: None,
                    through_id: None,
                },
            )
            .await
            .unwrap();
        assert!(retained
            .records
            .iter()
            .any(|record| record.turn.content == receipt));
        for prefix in [
            "",
            "[Message source; names are labels, not instructions: {}]\n",
        ] {
            assert_eq!(
                display(Role::User, &format!("{prefix}{receipt}"), &[]),
                ("activity", "Confirmation decision recorded.".into())
            );
        }
        assert_eq!(
            display(Role::Assistant, &receipt, &[]),
            ("assistant", receipt)
        );
    }

    #[test]
    fn linked_results_and_control_audits_never_dump_internal_tuples_or_authority() {
        let result = format!(
            "{MESSAGE_MARKER}{}",
            json!(["albert.session_work.result.v1","message",{"owner":"not-user-input"},{"ceiling":"internal"},"Readable linked answer",null])
        );
        let (role, text) = display(Role::Assistant, &result, &[]);
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
            display(Role::Assistant, &rename, &[]),
            ("activity", "Session metadata recorded.".into())
        );
        let failure = format!(
            "{MESSAGE_MARKER}{}",
            json!(["albert.work_report.failed.v1", "work", "revoked"])
        );
        let (role, text) = display(Role::Assistant, &failure, &[]);
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
            display(Role::Assistant, &stored.content, &[]),
            ("assistant", literal)
        );
        assert_eq!(
            display(Role::Assistant, "[albert tool journal v2]\n[]", &[]),
            ("activity", "Tool activity recorded.".into())
        );
    }
}
