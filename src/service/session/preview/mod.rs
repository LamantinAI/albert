//! Read-only native opening preview. A bounded tail is not a whole-history dump.
use octo_history::PageRequest;
use tracing::warn;

use super::{route_key, SessionService};
use crate::{
    db::repo::route::find,
    history::{preview_turn, HistoryStore, PreviewContent},
    sessions::{error::SessionResult, SessionContext},
};

#[derive(Debug)]
enum Preview {
    Text {
        text: String,
        record: i64,
        through: i64,
        scanned: usize,
        omitted: bool,
    },
    Empty,
    Service {
        through: i64,
        scanned: usize,
        older: bool,
    },
    Media {
        through: i64,
        scanned: usize,
        older: bool,
    },
    Unreadable,
    Stale,
}
impl SessionService {
    pub async fn current_view(&self, context: &SessionContext) -> SessionResult<bool> {
        let route = find(self.database.pool(), &route_key(context)?).await?;
        Ok(context.is_live()
            && route.is_some_and(|route| {
                route.session_id == context.session_id().as_str()
                    && Some(route.revision) == context.selector_revision()
            }))
    }
    /// The returned text is separately fenced at Telegram delivery by a host
    /// NavigationNotice marker and this same presentation's selector revision.
    pub async fn open_notice(&self, context: &SessionContext) -> String {
        let preview = match self.load_preview(context).await {
            Ok(preview) => preview,
            Err(error) => {
                warn!(%error, "native open preview unavailable");
                Preview::Unreadable
            }
        };
        render(
            context,
            preview,
            self.settings.limits.open_preview_max_bytes,
        )
    }
    async fn load_preview(&self, context: &SessionContext) -> SessionResult<Preview> {
        context.require_private()?;
        self.validate_context(context).await?;
        if !self.current_view(context).await? {
            return Ok(Preview::Stale);
        }
        let mut before = None;
        let mut through = None;
        let mut scanned = 0;
        let mut media = false;
        let mut unreadable = false;
        let mut older = false;
        let mut visible = None;
        for _ in 0..self.settings.limits.open_preview_max_records {
            let page = self
                .router
                .journal_page(
                    &context.root_history_key(),
                    PageRequest {
                        before_id: before,
                        through_id: through,
                        limit: 1,
                    },
                )
                .await?;
            through = Some(page.through_id);
            older = page.has_more;
            let Some(record) = page.records.first() else {
                break;
            };
            scanned += 1;
            match preview_turn(&record.turn) {
                PreviewContent::Text(text) if !text.trim().is_empty() => {
                    visible = Some(Preview::Text {
                        text,
                        record: record.id,
                        through: page.through_id,
                        scanned,
                        omitted: media || unreadable,
                    });
                    break;
                }
                PreviewContent::MediaOnly => media = true,
                PreviewContent::Unreadable => unreadable = true,
                _ => {}
            }
            before = page.next_before;
            if !page.has_more {
                break;
            }
        }
        // Neither a concurrent navigation nor an ended temporary view may be
        // represented as the currently opened chat by this stale receipt.
        if !self.current_view(context).await? {
            return Ok(Preview::Stale);
        }
        if let Some(text) = visible {
            return Ok(text);
        }
        if unreadable {
            return Ok(Preview::Unreadable);
        }
        if scanned == 0 {
            return Ok(Preview::Empty);
        }
        if media {
            return Ok(Preview::Media {
                through: through.unwrap_or(0),
                scanned,
                older,
            });
        }
        Ok(Preview::Service {
            through: through.unwrap_or(0),
            scanned,
            older,
        })
    }
}

fn render(context: &SessionContext, preview: Preview, max: usize) -> String {
    let opened = format!("Chat opened: {}.", context.session_id());
    match preview {
        Preview::Text { text, record, through, scanned, omitted } => {
            let head = format!("{opened}\n\nLatest recorded message (record {record}; snapshot {through}; scanned {scanned}):\n\n");
            let note = if omitted { "\n[Newer media/unreadable content omitted.]" } else { "" };
            let room = max.saturating_sub(head.len() + note.len());
            if text.len() <= room { return format!("{head}{text}{note}"); }
            let suffix = "\n[Preview truncated.]";
            let mut end = room.saturating_sub(suffix.len()).min(text.len()); while !text.is_char_boundary(end) { end -= 1; }
            format!("{head}{}{suffix}{note}", &text[..end])
        },
        Preview::Empty => format!("{opened}\n\nNo message records are stored for this chat yet."),
        Preview::Service { through, scanned, older } => format!("{opened}\n\nOnly service/tool records were found in the last {scanned} records (snapshot {through}).{}", if older { " Older messages may exist." } else { " No ordinary message is available to preview." }),
        Preview::Media { through, scanned, older } => format!("{opened}\n\nRecent records contain media; media previews are omitted (snapshot {through}; scanned {scanned}).{}", if older { " Older text may exist." } else { " No ordinary text message is available to preview." }),
        Preview::Unreadable => format!("{opened}\n\nHistory preview is currently unavailable; this does not mean the chat is empty."),
        Preview::Stale => "The selected chat changed while preparing this notice. Use /sessions to check the current selection.".into(),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use octo_history::{SessionStore, SqliteHistory};
    use rig::completion::Message;
    use serde_json::to_value;
    use sqlx::sqlite::SqlitePoolOptions;

    use super::SessionService;
    use crate::{
        history::{assistant_turn, tool_trace, HistoryStore, Turn, ACTION_MARKER},
        sessions::{
            settings::{SessionLimits, SessionSettings},
            Admission, AdmissionScope, ExternalIdentity, ReplyRoute, SessionContext,
        },
    };
    async fn fixture(records: usize, bytes: usize) -> (SessionService, SessionContext) {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        let service = SessionService::from_history(
            Arc::new(SqliteHistory::from_pool(pool).await.unwrap()),
            SessionSettings {
                limits: SessionLimits {
                    open_preview_max_records: records,
                    open_preview_max_bytes: bytes,
                    ..SessionLimits::default()
                },
                ..SessionSettings::default()
            },
        )
        .await
        .unwrap();
        let context = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("console", "local").unwrap(),
                    ReplyRoute::new("console", "stdin", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        (service, context)
    }
    #[tokio::test]
    async fn latest_meaningful_preview_is_utf8_bounded_and_never_mutates_journal() {
        let (service, context) = fixture(8, 256).await;
        let trace = tool_trace(&[Message::user("internal tool output must not appear")]).unwrap();
        let visible = assistant_turn(format!(
            "VISIBLE {}{ACTION_MARKER}hidden action log",
            "ёж ".repeat(200)
        ));
        service
            .router
            .append(
                &context.root_history_key(),
                &[Turn::user("older user message"), visible, trace],
            )
            .await
            .unwrap();
        let before = service
            .history
            .load_session(&context.journal())
            .await
            .unwrap();
        let notice = service.open_notice(&context).await;
        assert!(notice.contains("VISIBLE"));
        assert!(notice.contains("Preview truncated"));
        assert!(notice.contains("snapshot 3"));
        assert!(notice.contains(".\n\nLatest recorded message"));
        assert!(notice.contains("scanned 2):\n\nVISIBLE"));
        assert!(notice.len() <= 256);
        assert!(!notice.contains("internal tool output"));
        assert!(!notice.contains("hidden action log"));
        assert!(!notice.contains("older user message"));
        assert_eq!(
            to_value(before).unwrap(),
            to_value(
                service
                    .history
                    .load_session(&context.journal())
                    .await
                    .unwrap()
            )
            .unwrap()
        );
    }
    #[tokio::test]
    async fn bounded_tail_distinguishes_service_media_and_genuinely_empty() {
        let (service, context) = fixture(2, 2048).await;
        assert!(service
            .open_notice(&context)
            .await
            .contains("No message records"));
        let trace = tool_trace(&[Message::user("service")]).unwrap();
        service
            .router
            .append(
                &context.root_history_key(),
                &[
                    Turn::user("older text outside scan"),
                    trace.clone(),
                    trace.clone(),
                    trace,
                ],
            )
            .await
            .unwrap();
        let notice = service.open_notice(&context).await;
        assert!(notice.contains("Only service/tool"));
        assert!(notice.contains("last 2 records"));
        assert!(notice.contains("Older messages may exist"));
        assert!(!notice.contains("older text outside scan"));
        service
            .router
            .append(
                &context.root_history_key(),
                &[Turn::user("(sent an image, image/png)")],
            )
            .await
            .unwrap();
        let notice = service.open_notice(&context).await;
        assert!(notice.contains("media previews are omitted"));
        assert!(!notice.contains("No message records"));
    }
    #[tokio::test]
    async fn stale_selection_and_foreign_actor_cannot_display_history_as_current() {
        let (service, context) = fixture(4, 2048).await;
        service
            .router
            .append(
                &context.root_history_key(),
                &[Turn::assistant("private source answer")],
            )
            .await
            .unwrap();
        let next = service.create_private(&context, "Next").await.unwrap();
        let stale = service.open_notice(&context).await;
        assert!(stale.contains("selected chat changed"));
        assert!(!stale.contains("private source answer"));
        assert!(service
            .open_notice(&next)
            .await
            .contains("No message records"));
        let other = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("console", "other").unwrap(),
                    ReplyRoute::new("console", "other", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        assert!(service
            .open_private(
                &other,
                context.session_id(),
                other.selector_revision().unwrap()
            )
            .await
            .is_err());
        assert!(!service
            .open_notice(&other)
            .await
            .contains("private source answer"));
    }
    #[tokio::test]
    async fn read_failure_is_not_presented_as_empty_history() {
        let (service, context) = fixture(4, 2048).await;
        service.database.pool().close().await;
        let notice = service.open_notice(&context).await;
        assert!(notice.contains("preview is currently unavailable"));
        assert!(!notice.contains("No message records"));
    }
}
