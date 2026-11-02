mod model;
#[cfg(test)]
mod provider_fixture;
pub(crate) use model::ObservedModel;

use rand::random;

use super::{progress::FeedUpdate, StatusFeed};
use crate::api::models::TextDelta;

pub(crate) const MAX_TEXT_BYTES: usize = 256 * 1024;
#[derive(Clone)]
pub(crate) struct TextUpdate(pub TextDelta);
pub(super) struct TextCursor {
    generation: String,
    offset: usize,
    truncated: bool,
}

#[cfg(test)]
mod tests {
    use super::{provider_fixture::Server, ObservedModel, TextUpdate};
    use crate::{
        codex_http::CodexHttp,
        codex_model::CodexResponsesModel,
        db::repo::identity::bind,
        openrouter_http::OpenRouterHttp,
        service::session::{web::WebActor, SessionService},
        sessions::{actor::ActorToolPolicy, settings::SessionSettings, UserId},
        status::StatusFeed,
    };
    use futures::stream::unfold;
    use octo_core::{ConnectorId, EventBus, Filter, InProcessBus, SubscribeOptions};
    use octo_history::SqliteHistory;
    use rig::{
        agent::AgentBuilder,
        client::CompletionClient,
        completion::{
            CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Message,
            Prompt,
        },
        providers::{openai::Client, openrouter::Client as ApiClient},
        streaming::{RawStreamingChoice, StreamingCompletionResponse},
    };
    use serde_json::to_value;
    use std::sync::Arc;
    use tempfile::{tempdir, TempDir};
    use tokio::{
        spawn,
        sync::Notify,
        time::{timeout, Duration},
    };

    #[derive(Clone)]
    struct Model(Arc<Notify>);
    impl CompletionModel for Model {
        type Response = ();
        type StreamingResponse = ();
        type Client = Self;
        fn make(client: &Self, _: impl Into<String>) -> Self {
            client.clone()
        }
        async fn completion(
            &self,
            _: CompletionRequest,
        ) -> Result<CompletionResponse<()>, CompletionError> {
            panic!("observed completion must stream")
        }
        async fn stream(
            &self,
            _: CompletionRequest,
        ) -> Result<StreamingCompletionResponse<()>, CompletionError> {
            let gate = self.0.clone();
            Ok(StreamingCompletionResponse::stream(Box::pin(unfold(
                (0, gate),
                |(stage, gate)| async move {
                    let value = match stage {
                        0 => RawStreamingChoice::ReasoningDelta {
                            id: None,
                            reasoning: "private-reasoning-canary".into(),
                        },
                        1 => RawStreamingChoice::Message("Hello".into()),
                        2 => {
                            gate.notified().await;
                            RawStreamingChoice::FinalResponse(())
                        }
                        _ => return None,
                    };
                    Some((Ok(value), (stage + 1, gate)))
                },
            ))))
        }
    }
    async fn fixture_feed(bus: Arc<InProcessBus>) -> (TempDir, StatusFeed) {
        let dir = tempdir().unwrap();
        let history = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let service = SessionService::from_history(history, SessionSettings::default())
            .await
            .unwrap();
        let mut tx = service.history().begin_journal().await.unwrap();
        bind(&mut tx, "web", "account", "person").await.unwrap();
        tx.commit().await.unwrap();
        let actor = WebActor {
            account_id: "account".into(),
            user_id: UserId::parse("person").unwrap(),
            tools: ActorToolPolicy::ChatOnly,
        };
        let session = service.web_create(&actor, "Preview", false).await.unwrap();
        let context = service.web_context(&actor, &session.id).await.unwrap();
        let feed = StatusFeed::new(
            bus,
            ConnectorId::new("albert"),
            ConnectorId::new("web"),
            None,
        )
        .with_attempt("attempt".into())
        .with_session_context(Some(context));
        (dir, feed)
    }

    #[tokio::test]
    async fn first_visible_text_arrives_before_provider_final_and_excludes_reasoning() {
        let bus = Arc::new(InProcessBus::new(32));
        let mut events = bus
            .subscribe(Filter::by_kind("albert.text"), SubscribeOptions::default())
            .await
            .unwrap();
        let (_dir, feed) = fixture_feed(bus).await;
        let gate = Arc::new(Notify::new());
        let model = ObservedModel::new(Model(gate.clone()), Some(feed));
        let run = spawn(async move { AgentBuilder::new(model).build().prompt("Respond").await });
        let text = timeout(Duration::from_secs(2), async {
            loop {
                let event = events.next().await.unwrap();
                let update = &event.context.get::<TextUpdate>().unwrap().0;
                assert!(!update.text.contains("private-reasoning"));
                if update.operation == "append" {
                    break update.text.clone();
                }
            }
        })
        .await
        .unwrap();
        assert_eq!(text, "Hello");
        assert!(!run.is_finished());
        gate.notify_one();
        assert_eq!(
            timeout(Duration::from_secs(2), run)
                .await
                .unwrap()
                .unwrap()
                .unwrap(),
            "Hello"
        );
    }
    #[tokio::test]
    async fn subscription_observer_preserves_folded_tools_signatures_usage_and_rejects_eof() {
        for truncated in [false, true] {
            let server = Server::start(truncated).await;
            let client = Client::builder()
                .base_url(&server.url)
                .api_key("fixture-only")
                .http_client(CodexHttp::default())
                .build()
                .unwrap();
            let inner = CodexResponsesModel::make(&client, "fixture");
            let request = inner.completion_request(Message::user("Sample")).build();
            let bus = Arc::new(InProcessBus::new(32));
            let mut events = bus
                .subscribe(Filter::by_kind("albert.text"), SubscribeOptions::default())
                .await
                .unwrap();
            let (_dir, feed) = fixture_feed(bus).await;
            let model = ObservedModel::new(inner.clone(), Some(feed));
            let run = spawn(async move { model.completion(request).await });
            timeout(Duration::from_secs(2), async {
                loop {
                    let event = events.next().await.unwrap();
                    let update = &event.context.get::<TextUpdate>().unwrap().0;
                    assert!(!update.text.contains("canary"));
                    if update.operation == "append" {
                        assert_eq!(update.text, "Visible text");
                        break;
                    }
                }
            })
            .await
            .unwrap();
            assert!(!run.is_finished());
            server.gate.notify_one();
            let result = timeout(Duration::from_secs(2), run).await.unwrap().unwrap();
            if truncated {
                assert!(result
                    .unwrap_err()
                    .to_string()
                    .contains("ALBERT_TRANSPORT_INCOMPLETE"));
                continue;
            }
            let observed = result.unwrap();
            server.gate.notify_one();
            let request = inner.completion_request(Message::user("Sample")).build();
            let baseline = timeout(Duration::from_secs(2), inner.completion(request))
                .await
                .unwrap()
                .unwrap();
            assert_eq!(
                to_value(&observed.usage).unwrap(),
                to_value(&baseline.usage).unwrap()
            );
            assert_eq!(observed.usage.total_tokens, 42);
            assert_eq!(observed.usage.cached_input_tokens, 5);
            assert_eq!(observed.message_id.as_deref(), Some("message-item"));
            assert_eq!(observed.message_id, baseline.message_id);
            let mut choice = to_value(&observed.choice).unwrap();
            let mut expected = to_value(&baseline.choice).unwrap();
            for value in [&mut choice, &mut expected] {
                for item in value.as_array_mut().unwrap() {
                    if let Some(object) = item.as_object_mut() {
                        object.remove("internal_call_id");
                    }
                }
            }
            assert_eq!(choice, expected);
            let serialized = choice.to_string();
            for preserved in [
                "opaque-signature-canary",
                "private-summary-canary",
                "provider-call",
                "tool-item",
                "sample_tool",
                "Visible text",
            ] {
                assert!(serialized.contains(preserved), "missing {preserved}");
            }
        }
    }
    #[tokio::test]
    async fn compatible_api_requests_usage_and_keeps_the_empty_choice_terminal_chunk() {
        let server = Server::api().await;
        let client = ApiClient::builder()
            .base_url(&server.url)
            .api_key("fixture-only")
            .http_client(OpenRouterHttp::default())
            .build()
            .unwrap();
        let inner = client.completion_model("fixture");
        let request = inner.completion_request(Message::user("Sample")).build();
        let bus = Arc::new(InProcessBus::new(32));
        let mut events = bus
            .subscribe(Filter::by_kind("albert.text"), SubscribeOptions::default())
            .await
            .unwrap();
        let (_dir, feed) = fixture_feed(bus).await;
        let model = ObservedModel::new(inner, Some(feed));
        let run = spawn(async move { model.completion(request).await });
        timeout(Duration::from_secs(2), async {
            loop {
                let event = events.next().await.unwrap();
                let update = &event.context.get::<TextUpdate>().unwrap().0;
                if update.operation == "append" {
                    assert_eq!(update.text, "API text");
                    break;
                }
            }
        })
        .await
        .unwrap();
        assert!(!run.is_finished());
        server.gate.notify_one();
        let result = timeout(Duration::from_secs(2), run)
            .await
            .unwrap()
            .unwrap()
            .unwrap();
        assert_eq!(result.usage.input_tokens, 31);
        assert_eq!(result.usage.output_tokens, 11);
        assert_eq!(result.usage.total_tokens, 42);
        assert!(to_value(result.choice)
            .unwrap()
            .to_string()
            .contains("API text"));
    }
}

impl StatusFeed {
    pub(crate) fn text_enabled(&self) -> bool {
        self.feed.is_some()
            && self.execution_attempt.is_some()
            && self
                .session_context
                .as_ref()
                .is_some_and(|s| s.is_private())
            && self.is_live()
    }
    async fn text_event(
        &self,
        generation: &str,
        operation: &str,
        offset: usize,
        text: String,
        message_ids: Vec<String>,
    ) {
        let Some(attempt) = &self.execution_attempt else {
            return;
        };
        self.publish_line(
            "albert.text",
            String::new(),
            Some(FeedUpdate::Text(TextUpdate(TextDelta {
                attempt_id: attempt.clone(),
                generation: generation.into(),
                operation: operation.into(),
                offset,
                text,
                message_ids,
            }))),
        )
        .await;
    }
    pub(super) async fn text_begin(&self) -> String {
        let generation = format!("{:032x}", random::<u128>());
        *self.text.lock().unwrap() = Some(TextCursor {
            generation: generation.clone(),
            offset: 0,
            truncated: false,
        });
        self.text_event(&generation, "start", 0, String::new(), vec![])
            .await;
        generation
    }
    pub(super) async fn text_append(&self, generation: &str, text: &str) {
        let update = {
            let mut state = self.text.lock().unwrap();
            let Some(cursor) = state
                .as_mut()
                .filter(|s| s.generation == generation && !s.truncated)
            else {
                return;
            };
            let mut count = text.len().min(MAX_TEXT_BYTES.saturating_sub(cursor.offset));
            while !text.is_char_boundary(count) {
                count -= 1;
            }
            let offset = cursor.offset;
            cursor.offset += count;
            cursor.truncated = count < text.len();
            (offset, text[..count].to_owned(), cursor.truncated)
        };
        let mut rest = update.1.as_str();
        let mut offset = update.0;
        while !rest.is_empty() {
            let mut end = rest.len().min(4096);
            while !rest.is_char_boundary(end) {
                end -= 1;
            }
            self.text_event(generation, "append", offset, rest[..end].to_owned(), vec![])
                .await;
            offset += end;
            rest = &rest[end..];
        }
        if update.2 {
            self.text_event(generation, "truncated", update.0, String::new(), vec![])
                .await;
        }
    }
    pub(super) async fn text_reset(&self, generation: &str) {
        let current = self
            .text
            .lock()
            .unwrap()
            .as_ref()
            .is_some_and(|s| s.generation == generation);
        if current {
            self.text_event(generation, "reset", 0, String::new(), vec![])
                .await;
        }
    }
    pub(crate) async fn text_commit(&self, records: &[i64]) {
        let current = self
            .text
            .lock()
            .unwrap()
            .as_ref()
            .map(|s| (s.generation.clone(), s.offset));
        if let Some((generation, offset)) = current {
            self.text_event(
                &generation,
                "committed",
                offset,
                String::new(),
                records.iter().map(ToString::to_string).collect(),
            )
            .await;
        }
    }
}
