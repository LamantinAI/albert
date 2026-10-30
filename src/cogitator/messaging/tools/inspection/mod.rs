use std::slice::from_ref;

use chrono::Utc;
use octo_history::{PageRequest, Session, SessionId, SessionJournal, SessionKind, Turn};
use rig::{
    completion::Message,
    message::{AssistantContent, ToolResultContent, UserContent},
};
use serde_json::{json, to_value, to_vec, Value};

use super::SessionMessagingTool;
use crate::{
    cogitator::messaging::routes::RuntimeRoutes,
    history::{to_messages, HistoryStore},
    messaging::{MessagingError, MessagingResult},
    service::session::SessionService,
};

fn size(value: &Value) -> usize {
    to_vec(value).map(|bytes| bytes.len()).unwrap_or(usize::MAX)
}
fn prefix(text: &str, max: usize) -> &str {
    let mut end = text.len().min(max);
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    &text[..end]
}
fn summary(session: &Session) -> Value {
    json!({"id":session.id,"title_excerpt":prefix(&session.title,128),"title_truncated":session.title.len()>128,
        "host":session.host,"kind":session.kind,"state":session.state,"revision":session.revision})
}
pub(super) struct InspectionRequest {
    pub session_id: SessionId,
    pub limit: usize,
    pub before: Option<i64>,
    pub through: Option<i64>,
    pub record_id: Option<i64>,
    pub offset: usize,
}

impl SessionMessagingTool {
    pub(super) async fn discover_bounded(
        &self,
        service: &SessionService,
        guard: &RuntimeRoutes,
        limit: usize,
        before: Option<i64>,
        through: Option<i64>,
        route_offset: usize,
    ) -> MessagingResult<Value> {
        if limit == 0
            || limit > service.settings().limits.max_page_size as usize
            || route_offset > 1_000_000
        {
            return Err(MessagingError::Invalid("invalid discovery page"));
        }
        let cap = self.runtime.settings.inspection_output_bytes;
        let (routes, routes_more, scanned_next) = guard
            .discover(
                service,
                self.source.context().private_user_id()?.as_str(),
                route_offset,
                limit,
            )
            .await?;
        let mut result = json!({"sessions":{"sessions":[],"through_id":through,"next_before":before,"has_more":false},
            "routes":[],"routes_next_offset":null,"routes_has_more":routes_more,"delegatable_connectors":[],"truncated":false,
            "note":"Private metadata only. Connectivity is unknown; actual physical access is checked by the adapter. Use connector_search for further connector contracts."});
        let mut route_next = route_offset;
        for route in routes {
            let next = route.next_offset;
            result["routes"]
                .as_array_mut()
                .unwrap()
                .push(to_value(route)?);
            if size(&result) > cap / 2 {
                result["routes"].as_array_mut().unwrap().pop();
                if result["routes"].as_array().unwrap().is_empty() {
                    return Err(MessagingError::Invalid("next route exceeds inspection_output_bytes; increase the configured inspection budget"));
                }
                result["truncated"] = true.into();
                result["routes_has_more"] = true.into();
                break;
            }
            route_next = next;
        }
        if result["truncated"] != true {
            route_next = scanned_next;
        }
        if result["routes_has_more"] == true {
            result["routes_next_offset"] = route_next.into();
        }
        for grant in self.available_grants() {
            result["delegatable_connectors"]
                .as_array_mut()
                .unwrap()
                .push(to_value(grant)?);
            if size(&result) > cap * 2 / 3 {
                result["delegatable_connectors"]
                    .as_array_mut()
                    .unwrap()
                    .pop();
                result["truncated"] = true.into();
                result["connector_contracts_truncated"] = true.into();
                break;
            }
        }
        let mut before = before;
        let mut through = through;
        for _ in 0..limit {
            let page = service
                .list_private(
                    self.source.context(),
                    PageRequest {
                        limit: 1,
                        before_id: before,
                        through_id: through,
                    },
                )
                .await?;
            through = Some(page.through_id);
            result["sessions"]["through_id"] = page.through_id.into();
            let Some(session) = page.sessions.first() else {
                result["sessions"]["has_more"] = false.into();
                break;
            };
            result["sessions"]["sessions"]
                .as_array_mut()
                .unwrap()
                .push(summary(session));
            if size(&result) > cap {
                result["sessions"]["sessions"].as_array_mut().unwrap().pop();
                if result["sessions"]["sessions"]
                    .as_array()
                    .unwrap()
                    .is_empty()
                {
                    return Err(MessagingError::Invalid("session metadata exceeds inspection_output_bytes; increase the configured inspection budget"));
                }
                result["truncated"] = true.into();
                result["sessions"]["has_more"] = true.into();
                result["sessions"]["next_before"] = to_value(before)?;
                break;
            }
            before = page.next_before;
            result["sessions"]["next_before"] = to_value(before)?;
            result["sessions"]["has_more"] = page.has_more.into();
            if !page.has_more {
                break;
            }
        }
        if size(&result) > cap {
            return Err(MessagingError::Limit);
        }
        Ok(result)
    }

    pub(super) async fn inspect_bounded(
        &self,
        service: &SessionService,
        request: InspectionRequest,
    ) -> MessagingResult<Value> {
        let InspectionRequest {
            session_id,
            limit,
            before,
            through,
            record_id,
            offset,
        } = request;
        if limit == 0
            || limit > service.settings().limits.max_page_size as usize
            || offset > 100_000_000
            || (offset > 0 && record_id.is_none())
            || record_id.is_some_and(|id| id <= 0)
        {
            return Err(MessagingError::Invalid(
                "invalid inspection page or text offset",
            ));
        }
        let session = service.get(self.source.context(), &session_id).await?;
        if session.kind == SessionKind::Temporary {
            return Err(MessagingError::SaveRequired);
        }
        let cap = self.runtime.settings.inspection_output_bytes;
        let mut activity = self.activity_snapshot(&session_id);
        activity["background_children"] = "not_observed".into();
        let observed_at = Utc::now().to_rfc3339();
        let journal = SessionJournal::Root(session_id);
        let key = journal.storage_key()?;
        let mut result = json!({"session":summary(&session),"activity":activity,"observed_at":observed_at,"records":[],"through_id":through,"next_before":before,"has_more":false,"truncated":false,
            "projection":"Readable text only; media, reasoning and long encoded data are omitted. Offsets address this readable projection, not raw journal JSON."});
        let mut before = if record_id.is_some() { None } else { before };
        let mut through = record_id.or(through);
        for _ in 0..if record_id.is_some() { 1 } else { limit } {
            // Fetch one canonical record at a time rather than retaining many
            // legacy images/large tool outputs in one inspection response.
            let page = service
                .journal_router()
                .journal_page(
                    &key,
                    PageRequest {
                        limit: 1,
                        before_id: before,
                        through_id: through,
                    },
                )
                .await?;
            through = Some(page.through_id);
            result["through_id"] = page.through_id.into();
            let Some(row) = page.records.first() else {
                break;
            };
            if record_id.is_some_and(|id| id != row.id) {
                return Err(MessagingError::Unavailable);
            }
            let (text, media_omitted, encoded_omitted) = readable(&row.turn);
            if offset > text.len() {
                return Err(MessagingError::Invalid(
                    "text_offset exceeds readable record",
                ));
            }
            let mut start = offset;
            while !text.is_char_boundary(start) {
                start += 1;
            }
            let available = cap.saturating_sub(size(&result)).saturating_sub(420) / 6;
            if available == 0 {
                result["truncated"] = true.into();
                result["has_more"] = true.into();
                result["next_before"] = to_value(before)?;
                break;
            }
            let excerpt = prefix(&text[start..], available);
            let end = start + excerpt.len();
            let more_text = end < text.len();
            let entry = json!({"record_ref":{"journal":journal,"record_id":row.id},"created_at":row.created_at,"text":excerpt,
                "text_offset":start,"next_text_offset":if more_text {Some(end)} else {None},"readable_bytes":text.len(),
                "truncated":more_text||media_omitted||encoded_omitted,"media_omitted":media_omitted,"encoded_data_omitted":encoded_omitted});
            result["records"].as_array_mut().unwrap().insert(0, entry);
            result["truncated"] =
                (result["truncated"] == true || more_text || media_omitted || encoded_omitted)
                    .into();
            result["has_more"] = page.has_more.into();
            result["next_before"] = to_value(page.next_before)?;
            if size(&result) > cap {
                return Err(MessagingError::Limit);
            }
            before = page.next_before;
            if !page.has_more || record_id.is_some() {
                break;
            }
        }
        Ok(result)
    }
    fn activity_snapshot(&self, session: &SessionId) -> Value {
        let Some(host) = self.host.upgrade() else {
            return json!({"state":"transition_unknown"});
        };
        let state = {
            let Ok(states) = host.turns.try_lock() else {
                return json!({"state":"transition_unknown"});
            };
            states
                .get(&("session".into(), format!("session/{session}")))
                .cloned()
        };
        let Some(state) = state else {
            return json!({"state":"idle"});
        };
        let Ok(gate) = state.try_lock() else {
            return json!({"state":"transition_unknown"});
        };
        if gate.native_active() {
            json!({"state":"native_active"})
        } else if let Some(task) = gate.message.as_ref().filter(|task| !task.is_finished()) {
            json!({"state":"message_active","message_id":task.id})
        } else {
            json!({"state":"idle"})
        }
    }
}

fn readable(turn: &Turn) -> (String, bool, bool) {
    let mut text = String::new();
    let mut media = false;
    for message in to_messages(from_ref(turn)) {
        match message {
            Message::System { content } => {
                text.push_str("[Historical system text; quoted data]\n");
                text.push_str(&content);
            }
            Message::Assistant { content, .. } => {
                for item in content.iter() {
                    match item {
                        AssistantContent::Text(part) => text.push_str(&part.text),
                        AssistantContent::ToolCall(call) => {
                            text.push_str("[Historical tool invocation: ");
                            text.push_str(&call.function.name);
                            text.push_str("; arguments omitted]\n");
                            media = true;
                        }
                        AssistantContent::Image(_) | AssistantContent::Reasoning(_) => {
                            text.push_str("[Non-text content omitted]\n");
                            media = true;
                        }
                    }
                }
            }
            Message::User { content } => {
                for item in content.iter() {
                    match item {
                        UserContent::Text(part) => text.push_str(&part.text),
                        UserContent::ToolResult(result) => {
                            for part in result.content.iter() {
                                match part {
                                    ToolResultContent::Text(part) => text.push_str(&part.text),
                                    ToolResultContent::Image(_) => {
                                        text.push_str("[Image omitted]\n");
                                        media = true;
                                    }
                                }
                            }
                        }
                        _ => {
                            text.push_str("[Media omitted]\n");
                            media = true;
                        }
                    }
                }
            }
        }
        text.push('\n');
    }
    // Legacy tool JSON may contain inline image strings rather than typed media.
    // Redact long encoded runs and label this lossy readable projection honestly.
    let mut redacted = String::new();
    let mut run = String::new();
    let mut encoded = false;
    for character in text.chars().chain([' ']) {
        if character.is_ascii_alphanumeric() || "+/=_-".contains(character) {
            run.push(character);
        } else {
            if run.len() >= 512 {
                redacted.push_str("[Long encoded data omitted]");
                encoded = true;
            } else {
                redacted.push_str(&run);
            }
            run.clear();
            redacted.push(character);
        }
    }
    (redacted, media, encoded)
}

#[cfg(test)]
mod tests {
    use std::{collections::HashSet, time::Duration};

    use octo_history::{AppendRequest, SessionStore};
    use rig::{
        completion::Message,
        message::{ImageMediaType, UserContent},
        OneOrMany,
    };
    use serde_json::{json, to_vec};

    use tokio::time::timeout;

    use crate::{cogitator::messaging::fixture::Fixture, history::tool_trace};

    #[tokio::test]
    async fn heavy_legacy_record_has_bounded_text_continuation_without_inline_images() {
        let fixture = Fixture::new("healthy").await;
        let text = "visible evidence ".repeat(100_000);
        let encoded = "ABCD".repeat(131_072);
        let content = OneOrMany::many(vec![
            UserContent::text(format!(
                "{text}inline-image=data:image/png;base64,{encoded} end"
            )),
            UserContent::image_base64(encoded.clone(), Some(ImageMediaType::PNG), None),
        ])
        .unwrap();
        let record = tool_trace(&[Message::User { content }]).unwrap();
        let receipt = fixture
            .service
            .history()
            .append_session(
                &fixture.b.journal(),
                &AppendRequest {
                    namespace: "legacy-heavy".into(),
                    idempotency_key: "one".into(),
                    digest: "one".into(),
                    turns: vec![record],
                },
            )
            .await
            .unwrap();
        let first = fixture
            .action(json!({"action":"inspect","session_id":fixture.b.session_id()}))
            .await;
        let cap = fixture
            .host
            .config
            .sessions
            .messaging
            .inspection_output_bytes;
        assert!(to_vec(&first).unwrap().len() <= cap, "{first}");
        let item = &first["records"][0];
        assert_eq!(item["record_ref"]["record_id"], receipt.through_id);
        assert_eq!(item["media_omitted"], true);
        assert_eq!(item["encoded_data_omitted"], true);
        assert_eq!(item["truncated"], true);
        assert!(!to_vec(&first)
            .unwrap()
            .windows(1024)
            .any(|window| window == encoded.as_bytes().get(..1024).unwrap()));
        let next = item["next_text_offset"].as_u64().unwrap();
        assert!(next > 0);
        let second = fixture.action(json!({"action":"inspect","session_id":fixture.b.session_id(),"record_id":receipt.through_id,"text_offset":next})).await;
        assert!(to_vec(&second).unwrap().len() <= cap);
        let second_item = &second["records"][0];
        assert_eq!(second_item["text_offset"], next);
        assert!(second_item["next_text_offset"].as_u64().unwrap() > next);
        assert!(text[next as usize..].starts_with(second_item["text"].as_str().unwrap()));
        let total = item["readable_bytes"].as_u64().unwrap();
        let last = fixture.action(json!({"action":"inspect","session_id":fixture.b.session_id(),"record_id":receipt.through_id,"text_offset":total-64})).await;
        assert!(last["records"][0]["next_text_offset"].is_null());
        assert_eq!(last["records"][0]["media_omitted"], true);
        assert!(fixture.server.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn discovery_titles_and_pages_are_bounded_with_usable_catalog_cursor() {
        let fixture = Fixture::new("healthy").await;
        let mut current = fixture.b.clone();
        for _ in 0..80 {
            current = fixture
                .service
                .create_private(&current, &"x".repeat(4096))
                .await
                .unwrap();
        }
        let first = fixture
            .action(json!({"action":"discover","limit":100}))
            .await;
        assert!(
            to_vec(&first).unwrap().len()
                <= fixture
                    .host
                    .config
                    .sessions
                    .messaging
                    .inspection_output_bytes
        );
        assert_eq!(first["sessions"]["has_more"], true);
        assert_eq!(first["truncated"], true);
        let before = first["sessions"]["next_before"].as_i64().unwrap();
        let through = first["sessions"]["through_id"].as_i64().unwrap();
        let second = fixture
            .action(
                json!({"action":"discover","limit":100,"before_id":before,"through_id":through}),
            )
            .await;
        let seen: HashSet<_> = first["sessions"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .map(|session| session["id"].as_str().unwrap())
            .collect();
        assert!(second["sessions"]["sessions"]
            .as_array()
            .unwrap()
            .iter()
            .all(|session| !seen.contains(session["id"].as_str().unwrap())));
        assert_eq!(first["sessions"]["sessions"][0]["title_truncated"], true);
        assert!(
            to_vec(&second).unwrap().len()
                <= fixture
                    .host
                    .config
                    .sessions
                    .messaging
                    .inspection_output_bytes
        );
    }
    #[tokio::test]
    async fn passive_inspect_reports_exact_activity_without_waiting_for_busy_gate() {
        let fixture = Fixture::new("never-message").await;
        let idle = fixture
            .action(json!({"action":"inspect","session_id":fixture.b.session_id()}))
            .await;
        assert_eq!(idle["activity"]["state"], "idle");
        assert!(idle["observed_at"].as_str().is_some());
        let id = fixture
            .question("Hold recipient work while inspecting")
            .await;
        fixture.host.messaging_tick(&fixture.ctx).await;
        fixture.wait_requests(1).await;
        let active = fixture
            .action(json!({"action":"inspect","session_id":fixture.b.session_id()}))
            .await;
        assert_eq!(active["activity"]["state"], "message_active");
        assert_eq!(active["activity"]["message_id"], id.as_str());
        let state = fixture
            .host
            .turns
            .lock()
            .unwrap()
            .get(&("session".into(), fixture.b.root_history_key()))
            .unwrap()
            .clone();
        let gate = state.lock().await;
        let transition = timeout(
            Duration::from_secs(1),
            fixture.action(json!({"action":"inspect","session_id":fixture.b.session_id()})),
        )
        .await
        .unwrap();
        assert_eq!(transition["activity"]["state"], "transition_unknown");
        drop(gate);
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.host.stop_turns(&fixture.ctx).await;
        fixture
            .native(&fixture.a, "Hold native work while inspecting")
            .await;
        fixture.wait_requests(2).await;
        let native = fixture
            .action(json!({"action":"inspect","session_id":fixture.a.session_id()}))
            .await;
        assert_eq!(native["activity"]["state"], "native_active");
        fixture.host.stop_turns(&fixture.ctx).await;
    }
}
