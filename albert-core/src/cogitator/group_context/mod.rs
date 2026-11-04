//! One canonical group journal, with a deliberately public model-context view.
//! Internal tool records remain retained but never become another actor's prompt.
use octo_history::{ContextWindow, PageRequest, SessionAccess, SessionStore, StoredTurn};
use sha2::{Digest, Sha256};
use sqlx::{query, query_scalar};

use super::AlbertCogitator;
use crate::{
    history::sessions::parse_journal,
    history::{assistant_turn, preview_turn, PreviewContent, Role, Turn},
    messaging::contract::MESSAGE_MARKER,
};

fn public_turn(turn: &Turn) -> Option<Turn> {
    // Inter-session bookkeeping may carry private excerpts. It is not ordinary
    // group-visible speech, and group send/inspect is prohibited elsewhere too.
    if turn.role == Role::Assistant && turn.content.starts_with(MESSAGE_MARKER) {
        return None;
    }
    match preview_turn(turn) {
        PreviewContent::Text(text) => Some(if turn.role == Role::User {
            Turn::user(text)
        } else {
            assistant_turn(text)
        }),
        _ => None,
    }
}

fn digest(text: &str) -> String {
    format!("{:x}", Sha256::digest(text.as_bytes()))
}

impl AlbertCogitator {
    pub(super) async fn group_channel(&self, channel: &str) -> Result<bool, String> {
        let (Some(service), Ok(journal)) = (&self.session_service, parse_journal(channel)) else {
            return Ok(false);
        };
        Ok(service
            .history()
            .get_session(journal.session_id())
            .await
            .map_err(|e| e.to_string())?
            .is_some_and(|session| matches!(session.access, SessionAccess::GroupBound { .. })))
    }

    pub(super) async fn context_window(
        &self,
        channel: &str,
        before: Option<i64>,
    ) -> Result<ContextWindow, String> {
        let mut raw = self
            .history
            .context(channel)
            .await
            .map_err(|e| e.to_string())?;
        if !self.group_channel(channel).await? {
            if let Some(boundary) = before {
                raw.messages.retain(|m| m.id <= boundary);
            }
            return Ok(raw);
        }
        let service = self
            .session_service
            .as_ref()
            .ok_or("Group history is unavailable.")?;
        let journal = parse_journal(channel).map_err(|e| e.to_string())?;
        let limit = before.unwrap_or_else(|| raw.through_id());
        let compact = if let Some(compact) = raw.compact.filter(|c| c.through_id <= limit) {
            let marked: i64 = query_scalar("SELECT COUNT(*) FROM albert_group_public_compacts WHERE session_id=? AND compact_id=? AND through_id=? AND content_digest=?")
                .bind(journal.session_id().as_str()).bind(compact.id).bind(compact.through_id).bind(digest(&compact.content)).fetch_one(service.database().pool()).await.map_err(|e|e.to_string())?;
            (marked == 1).then_some(compact)
        } else {
            None
        };
        let start = compact.as_ref().map(|c| c.through_id).unwrap_or(0);
        let mut request = PageRequest {
            limit: 100,
            before_id: None,
            through_id: Some(limit),
        };
        let mut messages = Vec::new();
        loop {
            let page = service
                .history()
                .session_page(&journal, request)
                .await
                .map_err(|e| e.to_string())?;
            let mut reached = false;
            for record in &page.records {
                if record.id <= start {
                    reached = true;
                    continue;
                }
                if let Some(turn) = public_turn(&record.turn) {
                    messages.push(StoredTurn {
                        id: record.id,
                        turn,
                    });
                }
            }
            if reached || !page.has_more {
                break;
            }
            let Some(cursor) = page.next_before else {
                return Err("Group history cursor is unavailable.".into());
            };
            request = PageRequest {
                limit: 100,
                before_id: Some(cursor),
                through_id: Some(limit),
            };
        }
        messages.sort_by_key(|m| m.id);
        Ok(ContextWindow { compact, messages })
    }

    pub(super) async fn mark_group_compact(
        &self,
        channel: &str,
        through: i64,
        content: &str,
    ) -> Result<(), String> {
        if !self.group_channel(channel).await? {
            return Ok(());
        }
        let service = self
            .session_service
            .as_ref()
            .ok_or("Group history is unavailable.")?;
        let journal = parse_journal(channel).map_err(|e| e.to_string())?;
        let window = self
            .history
            .context(channel)
            .await
            .map_err(|e| e.to_string())?;
        if let Some(compact) = window
            .compact
            .filter(|c| c.through_id == through && c.content == content)
        {
            query("INSERT OR IGNORE INTO albert_group_public_compacts(session_id,compact_id,through_id,content_digest) VALUES (?,?,?,?)")
                .bind(journal.session_id().as_str()).bind(compact.id).bind(through).bind(digest(content)).execute(service.database().pool()).await.map_err(|e|e.to_string())?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use rig::completion::Message;

    use super::public_turn;
    use crate::history::{assistant_turn, tool_trace, Turn};

    #[test]
    fn public_view_excludes_hidden_work_but_retains_spoken_text_and_quoted_markers() {
        let hidden = tool_trace(&[Message::user("private tool result sentinel")]).unwrap();
        assert!(public_turn(&hidden).is_none());
        let spoken = assistant_turn(
            "Public reply\n\n[actions taken this turn]private action sentinel".into(),
        );
        assert_eq!(public_turn(&spoken).unwrap().content, "Public reply");
        let quoted = assistant_turn("[albert tool journal v2]\nquoted text".into());
        assert!(public_turn(&quoted)
            .unwrap()
            .content
            .contains("quoted text"));
        assert!(public_turn(&Turn::user("Public question")).is_some());
    }
    #[tokio::test]
    async fn unmarked_compact_is_ignored_and_original_public_records_remain_available() {
        use crate::{cogitator::testing::sessions::Fixture, history::HistoryStore};
        use octo_history::{CreateSession, SessionAccess, SessionStore};
        let fixture = Fixture::new().await;
        let mut create = CreateSession::private("");
        create.access = SessionAccess::GroupBound {
            connector: "telegram".into(),
            channel: "-42".into(),
        };
        let group = fixture
            .service
            .history()
            .create_session(create)
            .await
            .unwrap();
        let key = format!("session/{}", group.id);
        let hidden = tool_trace(&[Message::user("RAW-PRIVATE-TOOL-SENTINEL")]).unwrap();
        fixture
            .router
            .append(
                &key,
                &[
                    Turn::user("Public question"),
                    hidden,
                    Turn::assistant("Public answer"),
                ],
            )
            .await
            .unwrap();
        let raw = fixture.router.context(&key).await.unwrap();
        fixture
            .router
            .save_compact(
                &key,
                None,
                raw.through_id(),
                "UNSAFE-PRIVATE-COMPACT-SENTINEL",
            )
            .await
            .unwrap();
        let projected = fixture.host.context_window(&key, None).await.unwrap();
        assert!(projected.compact.is_none());
        assert_eq!(projected.messages.len(), 2);
        assert_eq!(projected.messages[0].turn.content, "Public question");
        assert_eq!(projected.messages[1].turn.content, "Public answer");
        let safe = "Public summary";
        let current = fixture.router.context(&key).await.unwrap();
        fixture
            .router
            .save_compact(&key, current.compact.map(|c| c.id), raw.through_id(), safe)
            .await
            .unwrap();
        fixture
            .host
            .mark_group_compact(&key, raw.through_id(), safe)
            .await
            .unwrap();
        let projected = fixture.host.context_window(&key, None).await.unwrap();
        assert_eq!(projected.compact.unwrap().content, safe);
        assert!(projected.messages.is_empty());
    }
}
