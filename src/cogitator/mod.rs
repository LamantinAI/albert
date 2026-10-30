//! Albert's cognition layer: memory, tools, reminders and scoped conversation turns.
//!
//! `chat.message` starts a rig tool loop with kaeru, scratchpad and connector tools.
//! `alarm.fired` handles user reminders or silent system routines.
//! SQL access carries native host authority independently of conversation tags.
//!
//! Owner-only ACL commands live in [`crate::acl`]; base routines in [`crate::routines`].

#[cfg(test)]
mod artifact_tests;
#[cfg(test)]
mod fixture;
mod hearing;
#[cfg(test)]
mod provider_fixture;
#[cfg(test)]
mod sql;

use std::{
    collections::HashMap,
    sync::{atomic::AtomicU64, Arc, Mutex},
    time::Duration,
};

use async_trait::async_trait;
use octo_approval::Authority;
use octo_core::{Cogitator, CogitatorContext, ConnectorId, Filter, OctoResult, Subscription};
use octo_openai_auth::SubscriptionAuth;
use tokio::{
    select, spawn,
    sync::Mutex as AsyncMutex,
    task::JoinSet,
    time::{interval, MissedTickBehavior},
};

use crate::{
    commands::{menu, publish_menu, SET_COMMANDS},
    config::Config,
    history::{sessions::SessionHistory, HistoryStore},
    memory::Memory,
    models::ModelPool,
    prompt::PromptFiles,
    routines::seed_base_routine,
    scratchpad::ScratchpadStore,
    service::session::SessionService,
    skills::SkillStore,
    subagents::Registry,
};

mod agent;
mod alarms;
mod build;
mod child;
mod compaction;
mod context;
mod errors;
mod feedback;
mod inbound;
mod output;
mod output_visibility;

mod messaging;
mod native_requests;
mod retention;
mod session_alarms;
pub(crate) mod session_dispatch;
mod session_runtime;
#[cfg(test)]
mod session_runtime_tests;
mod titles;
mod turns;
use self::{
    context::{
        action_context, catalog, channel_of, command_reply, incoming_context, now_rfc3339,
        scratchpad_scope, with_action_log, UserInput,
    },
    turns::{turn_key, ChannelState},
};

pub(crate) const SCHEDULER_ID: &str = "scheduler";
/// Payload marker distinguishing a system routine alarm from a user reminder.
pub(crate) const ROUTINE_MEMORY_REFLECTION: &str = "memory_reflection";

pub struct AlbertCogitator {
    authority: Authority,
    children: Registry,
    id: String,
    self_source: ConnectorId,
    config: Config,
    models: ModelPool,
    history: Arc<dyn HistoryStore>,
    session_service: Option<Arc<SessionService>>,
    session_history: Option<Arc<SessionHistory>>,
    request_gates: session_runtime::RequestGates,
    messaging: Option<Arc<messaging::MessagingRuntime>>,
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

#[async_trait]
impl Cogitator for AlbertCogitator {
    fn id(&self) -> &str {
        &self.id
    }

    fn filter(&self) -> Filter {
        // Perceive user messages and scheduler fires.
        Filter::by_kind("chat.message")
            .with_kind("alarm.fired")
            .with_kind("albert.session.closed")
            .with_kind("albert.presentation.detached")
            .with_kind("albert.presentation.opened")
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
        let mut messaging = interval(Duration::from_millis(
            self.config.sessions.messaging.poll_ms,
        ));
        messaging.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            select! {
                _ = messaging.tick(), if self.messaging.is_some() => { self.messaging_tick(&ctx).await; },
                next = subscription.next() => match next {
                    Some(envelope) if matches!(envelope.kind.as_str(), "albert.session.closed" | "albert.presentation.detached" | "albert.presentation.opened") => {
                        let me = self.clone(); let context = ctx.clone();
                        maintenance.spawn(async move { me.handle(envelope, &context).await; });
                    }
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
    use std::{sync::Arc, time::Duration};

    use octo_core::{
        CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
        EventKind, Filter, SubscribeOptions,
    };
    use rig::completion::Message;
    use serde_json::json;
    use tokio::{spawn, time::timeout};

    use super::{provider_fixture::setup, scratchpad_scope, AlbertCogitator};
    use crate::status::StatusFeed;

    async fn ask(agent: &Arc<AlbertCogitator>, ctx: &CogitatorContext, feed: StatusFeed) -> String {
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
        assert!(agent
            .scratchpad
            .render_scoped(&scratchpad_scope(&(String::new(), "room".into()), false))
            .contains("recorded once"));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(
            requests[1], requests[2],
            "only the failed continuation is retried"
        );
        assert!(requests.iter().all(|r| r["model"] == "writes"));
        assert!(serde_json::to_string(&feed.snapshot())
            .unwrap()
            .contains("recorded once"));
    }

    #[tokio::test]
    async fn ordinary_chat_has_no_extra_planner_call_or_created_scratchpad() {
        let (agent, ctx, server) = setup("healthy").await;
        let feed = StatusFeed::silent();
        assert_eq!(ask(&agent, &ctx, feed.clone()).await, "answer from healthy");
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert_eq!(feed.tool_call_count(), 0);
        assert!(agent
            .scratchpad
            .render_scoped(&scratchpad_scope(&(String::new(), "room".into()), false,))
            .contains("empty"));
    }
    #[tokio::test]
    async fn only_owner_turns_can_switch_and_current_loop_keeps_its_model() {
        for owner in [false, true] {
            let (agent, ctx, server) = setup("switches").await;
            let (answer, _) = timeout(
                Duration::from_secs(5),
                agent.run_agent(
                    &ctx,
                    "room",
                    "test",
                    Message::user("Switch to healthy"),
                    vec![],
                    None,
                    owner,
                    StatusFeed::silent(),
                    Some("switch-scope"),
                ),
            )
            .await
            .unwrap();
            let requests = server.requests.lock().unwrap().clone();
            let offered = requests[0]["tools"]
                .as_array()
                .unwrap()
                .iter()
                .any(|t| t["function"]["name"] == "model_select");
            assert_eq!(offered, owner);
            if owner {
                assert_eq!(agent.models.snapshot().selected, "healthy");
                assert_eq!(answer, "answer from switches");
                assert_eq!(requests.len(), 2);
                assert!(requests.iter().all(|r| r["model"] == "switches"));
                assert_eq!(
                    ask(&agent, &ctx, StatusFeed::silent()).await,
                    "answer from healthy"
                );
            } else {
                // The mock provider tried calling an unoffered tool anyway.
                assert_eq!(agent.models.snapshot().selected, "switches");
            }
        }
    }
    #[tokio::test]
    async fn model_file_tools_keep_the_chat_and_receive_delivery_confirmation() {
        for model in ["file-dispatch", "file-native"] {
            let (agent, original, server) = setup(model).await;
            let target = ConnectorId::new("telegram");
            let bus = original.bus();
            let mut requests = bus
                .subscribe(
                    Filter::by_target(target.clone()),
                    SubscribeOptions::default(),
                )
                .await
                .unwrap();
            let ctx = CogitatorContext::new(
                Default::default(),
                bus.clone(),
                vec![ConnectorInfo {
                    id: target.clone(),
                    capabilities: ConnectorCapabilities::bidirectional()
                        .with_emit_kinds([EventKind::new("chat.send_file.result")]),
                }],
            );
            let connector = spawn(async move {
                let req = timeout(Duration::from_secs(3), requests.next())
                    .await
                    .unwrap()
                    .unwrap();
                assert_eq!(req.kind.as_str(), "chat.send_file");
                assert_eq!(req.channel.as_ref().unwrap().as_str(), "-42");
                bus.publish(
                    Envelope::new(
                        ConnectorId::new("telegram"),
                        EventKind::new("chat.send_file.result"),
                        json!({"ok":true,"status":"sent","message_id":17}),
                    )
                    .with_correlation(req.id),
                )
                .await
                .unwrap();
            });
            let (answer, _) = timeout(
                Duration::from_secs(5),
                agent.run_agent(
                    &ctx,
                    "-42",
                    "test",
                    Message::user("send file"),
                    vec![],
                    Some(target),
                    false,
                    StatusFeed::silent(),
                    Some("file-scope"),
                ),
            )
            .await
            .unwrap();
            connector.await.unwrap();
            assert_eq!(answer, format!("answer from {model}"));
            let requests = server.requests.lock().unwrap();
            assert_eq!(requests.len(), 2);
            assert!(requests[1]["messages"].to_string().contains("message_id"));
        }
    }
    #[tokio::test]
    async fn connector_discovery_is_callable_without_duplicating_catalog_in_preamble() {
        let (agent, original, server) = setup("discovery").await;
        let ctx = CogitatorContext::new(
            Default::default(),
            original.bus(),
            vec![ConnectorInfo {
                id: ConnectorId::new("stockroom"),
                capabilities: ConnectorCapabilities::bidirectional()
                    .with_description("inventory.reserve { sku, quantity } reserves stock"),
            }],
        );
        assert_eq!(
            ask(&agent, &ctx, StatusFeed::silent()).await,
            "answer from discovery"
        );
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        assert!(!requests[0]["messages"]
            .to_string()
            .contains("inventory.reserve"));
        assert!(requests[0]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .any(|t| t["function"]["name"] == "connector_search"));
        assert!(requests[1]["messages"].to_string().contains("stockroom"));
        assert!(requests[1]["messages"]
            .to_string()
            .contains("inventory.reserve"));
    }
    #[tokio::test]
    async fn temporary_failure_after_tools_recovers_the_exact_request_without_replaying_tools() {
        let (agent, ctx, server) = setup("recovers").await;
        let feed = StatusFeed::silent();
        assert_eq!(
            ask(&agent, &ctx, feed.clone()).await,
            "answer from recovers"
        );
        assert_eq!(feed.tool_call_count(), 1);
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        assert_eq!(requests[1], requests[2]);
        assert!(requests[2]["messages"]
            .to_string()
            .contains("recorded once"));
        assert!(requests.iter().all(|r| r["model"] == "recovers"));
    }
}
