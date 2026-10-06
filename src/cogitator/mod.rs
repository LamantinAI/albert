//! `AlbertCogitator` — an Octo cogitator with **kaeru memory** + the **scheduler**
//! connector, wired into a reminder loop.
//!
//! It's the octolab `ReactCogitator` grown up: still a rig native tool-loop (the
//! task structure is agent-authored), but now it perceives two event kinds and carries persistent
//! memory:
//!
//! - `chat.message` → a normal turn. Tools: `dispatch_to_connector` (reaches the
//!   scheduler) + the kaeru memory verbs + the scratchpad.
//! - `alarm.fired`  → a user reminder is due (recall + remind) OR a system routine
//!   fired (silent self-care, e.g. memory reflection — see [`crate::routines`]).
//!
//! Owner-only ACL admin (`/allow` etc.) lives in [`crate::acl`]; the base routines
//! in [`crate::routines`].

#[cfg(test)]
mod fixture;
mod hearing;

use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex},
};

use async_trait::async_trait;
use octo_core::{Cogitator, CogitatorContext, ConnectorId, Filter, OctoResult, Subscription};
use octo_openai_auth::SubscriptionAuth;
use tokio::{select, spawn, sync::Mutex as AsyncMutex, task::JoinSet};

use crate::{
    commands::{menu, publish_menu, SET_COMMANDS},
    config::Config,
    history::HistoryStore,
    memory::Memory,
    models::ModelPool,
    prompt::PromptFiles,
    routines::seed_base_routine,
    scratchpad::ScratchpadStore,
    skills::SkillStore,
};

mod agent;
mod alarms;
mod context;
mod errors;
mod inbound;
mod output;

mod turns;
use self::{
    context::{
        action_context, catalog, channel_of, command_reply, incoming_context, now_rfc3339,
        with_action_log, UserInput,
    },
    turns::{turn_key, ChannelState},
};

pub(crate) const SCHEDULER_ID: &str = "scheduler";
/// Payload marker distinguishing a system routine alarm from a user reminder.
pub(crate) const ROUTINE_MEMORY_REFLECTION: &str = "memory_reflection";

pub struct AlbertCogitator {
    id: String,
    self_source: ConnectorId,
    config: Config,
    models: ModelPool,
    history: Arc<dyn HistoryStore>,
    memory: Memory,
    scratchpad: Arc<ScratchpadStore>,
    skills: Arc<SkillStore>,
    prompt: Arc<PromptFiles>,
    /// Shared, refresh-serialised ChatGPT-subscription auth — ONE refresh owner across
    /// the LLM backend and the voice (transcribe/speak) paths. See [`SubscriptionAuth`].
    auth: Arc<SubscriptionAuth>,
    /// Albert's channel interruption policy. Octo only carries control signals.
    turns: Mutex<HashMap<(String, String), Arc<AsyncMutex<ChannelState>>>>,
    /// Unique cancellation scope for each attempt.
    turn_seq: AtomicU64,
}

impl AlbertCogitator {
    pub fn new(
        id: impl Into<String>,
        config: Config,
        history: Arc<dyn HistoryStore>,
        memory: Memory,
        scratchpad: Arc<ScratchpadStore>,
        skills: Arc<SkillStore>,
        prompt: Arc<PromptFiles>,
        auth: Arc<SubscriptionAuth>,
    ) -> Arc<Self> {
        let id = id.into();
        let models = ModelPool::new(&config);
        Arc::new(Self {
            models,
            self_source: ConnectorId::new(format!("cogitator/{id}")),
            id,
            config,
            history,
            memory,
            scratchpad,
            skills,
            prompt,
            auth,
            turns: Mutex::new(HashMap::new()),
            turn_seq: AtomicU64::new(0),
        })
    }
}

#[async_trait]
impl Cogitator for AlbertCogitator {
    fn id(&self) -> &str {
        &self.id
    }

    fn filter(&self) -> Filter {
        // Perceive user messages and scheduler fires.
        Filter::by_kind("chat.message").with_kind("alarm.fired")
    }

    async fn run(
        self: Arc<Self>,
        ctx: CogitatorContext,
        mut subscription: Subscription,
    ) -> OctoResult<()> {
        // Seed Albert's base routines (memory reflection) once the scheduler is up.
        spawn(seed_base_routine(
            ctx.bus(),
            self.self_source.clone(),
            self.config.reflection_secs,
        ));
        // Publish the command menu on every channel that takes one.
        let channels: Vec<ConnectorId> = ctx
            .connectors()
            .iter()
            .filter(|c| {
                c.capabilities
                    .event_kinds_accept
                    .iter()
                    .any(|k| k.as_str() == SET_COMMANDS)
            })
            .map(|c| c.id.clone())
            .collect();
        if !channels.is_empty() {
            spawn(publish_menu(
                ctx.bus(),
                self.self_source.clone(),
                channels,
                menu(&self.skills.commands()),
            ));
        }
        let mut maintenance = JoinSet::new();
        loop {
            select! {
                next = subscription.next() => match next {
                    Some(envelope) if envelope.kind.as_str() == "alarm.fired" => {
                        let me = self.clone(); let context = ctx.clone();
                        maintenance.spawn(async move { me.on_alarm(envelope, &context).await; });
                    }
                    Some(envelope) => self.clone().handle(envelope, &ctx).await,
                    None => { self.stop_turns(&ctx).await; return Ok(()); },
                },
                _ = maintenance.join_next(), if !maintenance.is_empty() => {},
                _ = ctx.shutdown.cancelled() => { self.stop_turns(&ctx).await; return Ok(()); },
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::errors::{provider_status_code, token_rejected, user_facing_llm_error};
    use super::UserInput;
    use octo_core::Blob;
    use rig::{
        completion::{Message, PromptError},
        message::UserContent,
    };

    #[test]
    fn text_input_stays_a_plain_user_message() {
        let input = UserInput {
            text: "hello".into(),
            images: Vec::new(),
            seed: None,
            voice: None,
        };
        assert!(matches!(input.prompt(), Message::User { content } if content.len() == 1));
        assert_eq!(input.transcript(), "hello");
    }

    #[test]
    fn image_input_becomes_image_plus_caption() {
        let blob = Blob::new(vec![0xFFu8, 0xD8, 0xFF], "image/jpeg").with_filename("photo.jpg");
        let input = UserInput {
            text: "what's in the photo?".into(),
            images: vec![blob],
            seed: None,
            voice: None,
        };
        let Message::User { content } = input.prompt() else {
            panic!("expected a user message");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert_eq!(items.len(), 2);
        assert!(matches!(items[0], UserContent::Image(_)));
        assert!(matches!(&items[1], UserContent::Text(t) if t.text == "what's in the photo?"));
        assert!(input.transcript().contains("image/jpeg"));
    }

    #[test]
    fn an_album_becomes_every_image_plus_one_caption() {
        let images = vec![
            Blob::new(vec![1], "image/jpeg").with_filename("a.jpg"),
            Blob::new(vec![2], "image/png").with_filename("b.png"),
            Blob::new(vec![3], "image/jpeg").with_filename("c.jpg"),
        ];
        let input = UserInput {
            text: String::new(),
            images,
            seed: None,
            voice: None,
        };
        let Message::User { content } = input.prompt() else {
            panic!("expected a user message");
        };
        let items: Vec<_> = content.into_iter().collect();
        // Three image blocks, then a single caption — not one photo, not three captions.
        assert_eq!(items.len(), 4);
        assert!(items[0..3]
            .iter()
            .all(|i| matches!(i, UserContent::Image(_))));
        assert!(matches!(&items[3], UserContent::Text(t) if t.text.contains("these images")));
        assert_eq!(input.transcript(), "(sent 3 images)");
    }

    /// The forced-refresh retry keys off the LIVE provider response. The first shape
    /// is the real incident (2026-08-10): HTTP 401 `token_expired` while the JWT
    /// `exp` was still eight days out.
    #[test]
    fn token_rejection_is_detected_from_the_live_response() {
        use rig::completion::CompletionError;

        let incident = PromptError::CompletionError(CompletionError::ProviderError(
            "Invalid status code 401 Unauthorized with message: \
             {\"detail\":\"token_expired\"}"
                .into(),
        ));
        assert!(token_rejected(&incident));

        // Either signal alone suffices: a bare 401 (rig's SSE connect path strips
        // the body) or a token_expired that arrives without the status line.
        let bare_401 = PromptError::CompletionError(CompletionError::ProviderError(
            "Invalid status code: 401 Unauthorized".into(),
        ));
        assert!(token_rejected(&bare_401));
        let expired_only = PromptError::CompletionError(CompletionError::ProviderError(
            "response failed: token_expired".into(),
        ));
        assert!(token_rejected(&expired_only));

        // An unrelated provider failure must NOT trigger a forced refresh.
        let unrelated = PromptError::CompletionError(CompletionError::ProviderError(
            "Invalid status code 500 Internal Server Error with message: boom".into(),
        ));
        assert!(!token_rejected(&unrelated));
    }

    #[test]
    fn provider_errors_are_shown_politely_not_dumped() {
        use rig::completion::CompletionError;
        let err = |s: &str| {
            user_facing_llm_error(&PromptError::CompletionError(
                CompletionError::ProviderError(s.into()),
            ))
        };

        // OpenRouter-style body: the code is recovered from the JSON, and the raw blob
        // (message, request detail) never reaches the user.
        let openrouter = err("{\"error\":{\"message\":\"Provider returned error\",\"code\":400}}");
        assert_eq!(
            openrouter,
            "LLM provider error: 400. Please try again in a moment."
        );
        assert!(!openrouter.contains("Provider returned error"));

        // A textual status marker is honoured too.
        assert_eq!(
            err("Invalid status code 502 Bad Gateway"),
            "LLM provider error: 502. Please try again in a moment."
        );

        // No recoverable code -> a clean generic line, still no raw text leaked.
        let opaque = err("upstream connection reset by peer");
        assert_eq!(opaque, "LLM provider error. Please try again in a moment.");

        // A bare integer in range is never guessed as a status (would misreport).
        assert_eq!(provider_status_code("used 500 tokens of context"), None);
    }

    #[test]
    fn max_turns_is_its_own_message_not_a_provider_error() {
        let e = PromptError::MaxTurnsError {
            max_turns: 10,
            chat_history: Box::new(vec![]),
            prompt: Box::new(Message::user("x")),
        };
        let msg = user_facing_llm_error(&e);
        assert!(msg.contains("step budget"), "got: {msg}");
        assert!(!msg.contains("provider"), "got: {msg}");
    }

    #[test]
    fn captionless_image_gets_a_default_instruction() {
        let blob = Blob::new(vec![1u8, 2, 3], "image/png");
        let input = UserInput {
            text: "  ".into(),
            images: vec![blob],
            seed: None,
            voice: None,
        };
        let Message::User { content } = input.prompt() else {
            panic!("expected a user message");
        };
        let items: Vec<_> = content.into_iter().collect();
        assert!(matches!(&items[1], UserContent::Text(t) if t.text.contains("no caption")));
    }
}

#[cfg(test)]
mod model_pool_tests {
    use std::{
        sync::{Arc, Mutex},
        time::Duration,
    };

    use axum::{extract::State, http::StatusCode, routing::post, serve, Json, Router};
    use octo_core::CogitatorContext;
    use rig::completion::Message;
    use serde_json::{json, Value};
    use tokio::{net::TcpListener, spawn, task::JoinHandle, time::timeout};

    use super::{fixture::fixture, AlbertCogitator};
    use crate::{
        config::AuthMode,
        models::{ModelPool, ModelSpec, PoolConfig},
        status::StatusFeed,
    };

    struct Server {
        requests: Arc<Mutex<Vec<Value>>>,
        task: JoinHandle<()>,
        url: String,
    }
    impl Drop for Server {
        fn drop(&mut self) {
            self.task.abort();
        }
    }

    async fn completion(
        State(requests): State<Arc<Mutex<Vec<Value>>>>,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let model = body["model"].as_str().unwrap();
        let count = {
            let mut requests = requests.lock().unwrap();
            requests.push(body.clone());
            requests.iter().filter(|r| r["model"] == model).count()
        };
        if model == "hanging" {
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
        if model == "failing" || (model == "writes" && count > 1) {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({"error":{"code":503,"message":"overloaded"}})),
            );
        }
        if model == "incompatible" {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"error":{"code":400,"message":"This model does not support images"}})),
            );
        }
        let (message, finish) = if model == "writes" {
            (
                json!({"role":"assistant","content":null,"tool_calls":[{"id":"call_note","type":"function","function":{"name":"scratchpad_note","arguments":"{\"text\":\"recorded once\"}"}}]}),
                "tool_calls",
            )
        } else {
            (
                json!({"role":"assistant","content":if model == "empty" { " ".to_string() } else { format!("answer from {model}") }}),
                "stop",
            )
        };
        (
            StatusCode::OK,
            Json(
                json!({"id":"test-completion","object":"chat.completion","created":1,"model":model,
            "choices":[{"index":0,"message":message,"finish_reason":finish}],
            "usage":{"prompt_tokens":1,"completion_tokens":1,"total_tokens":2}}),
            ),
        )
    }

    async fn setup(first: &str) -> (Arc<AlbertCogitator>, CogitatorContext, Server) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let app = Router::new()
            .route("/chat/completions", post(completion))
            .with_state(requests.clone());
        let task = spawn(async move {
            serve(listener, app).await.unwrap();
        });
        let server = Server {
            requests,
            task,
            url,
        };
        let (mut agent, ctx, _) = fixture();
        let me = Arc::get_mut(&mut agent).unwrap();
        me.config.api_key = Some("local-test-key".into());
        me.config.models = Some(PoolConfig {
            default: first.into(),
            max_attempts: 3,
            retries_per_model: 0,
            retry_delay_ms: 0,
            models: [first, "healthy"]
                .into_iter()
                .map(|id| ModelSpec {
                    id: id.into(),
                    model: id.into(),
                    provider: AuthMode::ApiKey,
                    base_url: Some(server.url.clone()),
                    api_key_env: None,
                    vision: true,
                    tools: true,
                    request_timeout_ms: if id == "hanging" { 20 } else { 2000 },
                })
                .collect(),
        });
        me.models = ModelPool::new(&me.config);
        (agent, ctx, server)
    }

    async fn ask(agent: &AlbertCogitator, ctx: &CogitatorContext, feed: StatusFeed) -> String {
        timeout(
            Duration::from_secs(5),
            agent.run_agent(
                ctx,
                "room",
                "test",
                Message::user("continue"),
                vec![Message::user("earlier context")],
                None,
                false,
                feed,
                Some("test-scope"),
            ),
        )
        .await
        .unwrap()
        .0
    }

    #[tokio::test]
    async fn real_http_fallback_keeps_history_permissions_and_supports_live_selection() {
        for first in ["failing", "incompatible", "hanging", "empty"] {
            let (agent, ctx, server) = setup(first).await;
            // A completed perception call must not prevent fallback before model tools.
            let feed = StatusFeed::silent();
            feed.start_external(
                "heard",
                "dispatch_to_connector",
                json!({"target":"transcribe"}),
            );
            feed.finish_external("heard", "transcript");
            assert_eq!(ask(&agent, &ctx, feed).await, "answer from healthy");
            let requests = server.requests.lock().unwrap().clone();
            assert_eq!(requests.len(), 2);
            assert_eq!(requests[0]["messages"], requests[1]["messages"]);
            assert_eq!(requests[0]["tools"], requests[1]["tools"]);
            assert!(!requests[1]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == "restart"));
            assert!(requests[1]["messages"]
                .to_string()
                .contains("earlier context"));
            agent.models.command("healthy", true);
            assert_eq!(
                ask(&agent, &ctx, StatusFeed::silent()).await,
                "answer from healthy"
            );
            assert_eq!(server.requests.lock().unwrap().len(), 3);
        }
    }

    #[tokio::test]
    async fn provider_failure_after_a_real_tool_does_not_replay_the_action() {
        let (agent, ctx, server) = setup("writes").await;
        let feed = StatusFeed::silent();
        let answer = ask(&agent, &ctx, feed.clone()).await;
        assert!(answer.contains("fallback stopped"), "{answer}");
        assert_eq!(feed.tool_call_count(), 1);
        assert!(agent.scratchpad.render("room").contains("recorded once"));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(requests.iter().all(|r| r["model"] == "writes"));
        assert!(serde_json::to_string(&feed.snapshot())
            .unwrap()
            .contains("recorded once"));
    }
}
