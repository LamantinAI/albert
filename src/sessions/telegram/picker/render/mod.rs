use std::time::Instant;

use octo_connector_telegram::{InlineButton, InlineKeyboard};
use octo_core::Envelope;
use octo_history::{PageRequest, Session};

use super::registry::{Action, Binding, Cursor, Registry};
use crate::sessions::{
    error::{SessionError, SessionResult},
    telegram::{navigation::navigation_notice, policy::SessionTelegramPolicy},
    SessionContext,
};

fn label(session: &Session, index: usize, current: bool) -> String {
    let clean = session
        .title
        .chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect::<String>();
    let clean = clean.split_whitespace().collect::<Vec<_>>().join(" ");
    let title = if clean.is_empty() {
        "Unnamed chat"
    } else {
        &clean
    };
    let id = session.id.as_str();
    let short = &id[id.len().saturating_sub(6)..];
    let prefix = format!("{}. ", index + 1);
    let suffix = format!(" · {short}{}", if current { " (current)" } else { "" });
    let room = 64 - prefix.chars().count() - suffix.chars().count();
    let title = if title.chars().count() > room {
        format!("{}…", title.chars().take(room - 1).collect::<String>())
    } else {
        title.into()
    };
    format!("{prefix}{title}{suffix}")
}
impl SessionTelegramPolicy {
    pub(in crate::sessions::telegram) async fn picker_page(
        &self,
        context: &SessionContext,
        binding: Binding,
        cursor: Cursor,
    ) -> SessionResult<Envelope> {
        self.service.validate_context(context).await?;
        let page = self
            .service
            .list_private(
                context,
                PageRequest {
                    before_id: cursor.before,
                    through_id: cursor.through,
                    limit: self
                        .service
                        .settings()
                        .picker
                        .page_size
                        .min(self.service.settings().limits.max_page_size as usize),
                },
            )
            .await?;
        let cursor = Cursor {
            through: Some(page.through_id),
            ..cursor
        };
        let token = Registry::token();
        let mut actions = Vec::new();
        let mut rows = Vec::new();
        for (index, session) in page.sessions.iter().enumerate() {
            rows.push(vec![InlineButton::new(
                label(session, index, session.id == *context.session_id()),
                format!("sp:{token}:{}", actions.len()),
            )
            .map_err(|_| SessionError::Unavailable)?]);
            actions.push(Action::Open(session.id.clone()));
        }
        let mut navigation = Vec::new();
        if cursor.before.is_some() {
            let previous = self
                .picker
                .registry
                .lock()
                .map_err(|_| SessionError::Unavailable)?
                .previous(&cursor, &binding, Instant::now());
            let name = if previous.is_some() {
                "Previous page"
            } else {
                "First page"
            };
            navigation.push(
                InlineButton::new(name, format!("sp:{token}:{}", actions.len()))
                    .map_err(|_| SessionError::Unavailable)?,
            );
            actions.push(Action::Page(previous.unwrap_or_default()));
        }
        if let Some(before) = page.next_before.filter(|_| page.has_more) {
            navigation.push(
                InlineButton::new("Next page", format!("sp:{token}:{}", actions.len()))
                    .map_err(|_| SessionError::Unavailable)?,
            );
            actions.push(Action::Page(Cursor {
                before: Some(before),
                through: Some(page.through_id),
                previous: Some(token.clone()),
            }));
        }
        if !navigation.is_empty() {
            rows.push(navigation);
        }
        let text = if page.sessions.is_empty() {
            "No saved chats on this page. Use /new <name> to create one."
        } else {
            "Saved chats\n\nChoose a chat below. /open <session-id> is also available."
        };
        let mut reply = navigation_notice(context, text.into());
        if !rows.is_empty() {
            reply = reply
                .with_context(InlineKeyboard::new(rows).map_err(|_| SessionError::Unavailable)?);
            self.picker
                .registry
                .lock()
                .map_err(|_| SessionError::Unavailable)?
                .insert(token, binding, cursor, actions, Instant::now());
        }
        Ok(reply)
    }
}

#[cfg(test)]
mod tests {
    use super::label;
    use octo_history::{CreateSession, SessionStore, SqliteHistory};
    use tempfile::tempdir;

    #[tokio::test]
    async fn long_unicode_legacy_labels_are_plain_bounded_and_distinct_without_catalog_mutation() {
        let dir = tempdir().unwrap();
        let history = SqliteHistory::open_retained(dir.path().join("history.db"))
            .await
            .unwrap();
        let title = format!(
            " \n{}\t<b>literal name</b>",
            "Исследование памяти ".repeat(40)
        );
        let mut create = CreateSession::private("owner");
        create.title = title.clone();
        let first = history.create_session(create.clone()).await.unwrap();
        let second = history.create_session(create).await.unwrap();
        let a = label(&first, 0, true);
        let b = label(&second, 1, false);
        assert!(a.chars().count() <= 64 && b.chars().count() <= 64);
        assert!(a.contains("(current)"));
        assert!(a.contains("Исследование"));
        assert_ne!(a, b);
        assert!(!a.chars().any(char::is_control));
        assert_eq!(
            history.get_session(&first.id).await.unwrap().unwrap().title,
            title
        );
        let mut empty = first.clone();
        empty.title = "\t\n".into();
        assert!(label(&empty, 2, false).contains("Unnamed chat"));
    }
}

#[cfg(test)]
mod limited_page_tests {
    use std::{sync::Arc, time::Duration};

    use octo_approval::Authority;
    use octo_connector_telegram::{InlineKeyboard, NativeFrame};
    use octo_core::ConnectorId;
    use octo_history::SqliteHistory;
    use tempfile::tempdir;

    use crate::{
        service::session::SessionService,
        sessions::{
            settings::{SessionLimits, SessionSettings},
            telegram::{
                picker::registry::{Binding, Cursor},
                SessionTelegramPolicy,
            },
            Admission, AdmissionScope, ExternalIdentity, ReplyRoute,
        },
    };

    #[tokio::test]
    async fn valid_small_catalog_page_limit_still_produces_a_working_picker() {
        let dir = tempdir().unwrap();
        let service = Arc::new(
            SessionService::from_history(
                Arc::new(
                    SqliteHistory::open_retained(dir.path().join("history.db"))
                        .await
                        .unwrap(),
                ),
                SessionSettings {
                    limits: SessionLimits {
                        max_page_size: 1,
                        ..SessionLimits::default()
                    },
                    ..SessionSettings::default()
                },
            )
            .await
            .unwrap(),
        );
        let a = service
            .admit(
                Admission::new(
                    ExternalIdentity::new("telegram", "7").unwrap(),
                    ReplyRoute::new("telegram", "7", None).unwrap(),
                    AdmissionScope::Private,
                    "host",
                )
                .unwrap(),
            )
            .await
            .unwrap();
        let b = service.create_private(&a, "Current").await.unwrap();
        let native = NativeFrame {
            connector: ConnectorId::new("telegram"),
            bot_id: 99,
            update_id: 1,
            message_id: 1,
            date: 1,
            chat_id: 7,
            actor_id: Some(7),
            chat_type: "private".into(),
            command_text: Some("/sessions".into()),
            digest: "menu".into(),
            media: false,
        };
        let policy =
            SessionTelegramPolicy::new(service, Authority::new(Duration::from_secs(60), 16));
        let reply = policy
            .picker_page(&b, Binding::native(&native, &b).unwrap(), Cursor::default())
            .await
            .unwrap();
        let rows = reply.context.get::<InlineKeyboard>().unwrap().rows();
        assert_eq!(rows.len(), 2);
        assert!(rows[0][0].label().contains("Current"));
        assert_eq!(rows[1][0].label(), "Next page");
    }
}
