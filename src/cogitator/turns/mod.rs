//! Albert's interruption policy. A channel transition is serialized with reply
//! publication; only the model/tool future is aborted, never accepted input.

mod compact;
mod run;
mod spawn;

use std::sync::Arc;

use octo_core::{CogitatorContext, Envelope};
use octo_rig::carry_out_cancel;
use rig::completion::Message;
use tokio::task::JoinHandle;
use tracing::warn;

use super::{channel_of, AlbertCogitator};
use crate::{
    history::{journal_messages, tool_trace},
    status::StatusFeed,
};

#[derive(Default)]
pub(super) struct ChannelState {
    active: Option<ActiveTurn>,
}

struct ActiveTurn {
    /// Original task envelope; absent for a compact started while idle.
    continuation: Option<Arc<Envelope>>,
    compact_id: Option<i64>,
    scope: String,
    task: JoinHandle<()>,
    feed: StatusFeed,
    /// Full working context, including images and skill instructions, survives
    /// every interruption even when the persisted rolling history trims old turns.
    messages: Vec<Message>,
    owner: bool,
}

struct Interrupted {
    /// Original task envelope; absent for a compact started while idle.
    continuation: Option<Arc<Envelope>>,
    compact_id: Option<i64>,
    scope: String,
    messages: Vec<Message>,
    checkpoint: Vec<Message>,
    owner: bool,
}

impl ChannelState {
    async fn interrupt(&mut self) -> Option<Interrupted> {
        let active = self.active.take()?;
        active.task.abort();
        let _ = active.task.await; // no old hooks/replies may race the checkpoint
        let checkpoint = active.feed.checkpoint();
        let mut messages = active.messages;
        messages.extend(journal_messages(&checkpoint));
        Some(Interrupted {
            continuation: active.continuation,
            compact_id: active.compact_id,
            scope: active.scope,
            messages,
            checkpoint,
            owner: active.owner,
        })
    }
}

pub(super) fn turn_key(env: &Envelope) -> (String, String) {
    (env.source.to_string(), channel_of(env))
}

impl AlbertCogitator {
    pub(super) async fn cancel_channel(
        &self,
        key: &(String, String),
        ctx: &CogitatorContext,
    ) -> bool {
        let state = self.turns.lock().unwrap().get(key).cloned();
        let Some(state) = state else {
            return self.children.cancel_channel(key).await;
        };
        let mut locked = state.lock().await;
        let previous = locked.interrupt().await;
        if let Some(previous) = &previous {
            self.cancel_scope(&previous.scope, ctx).await;
        }
        let children_cancelled = self.children.cancel_channel(key).await;
        let Some(previous) = previous else {
            return children_cancelled;
        };
        // Caller records the explicit /cancel after this checkpoint.
        let channel = &key.1;
        if let Some(record) = tool_trace(&previous.checkpoint) {
            if let Err(error) = self.history.append(channel, &[record]).await {
                warn!(%error, "could not persist interrupted tool history");
            }
        }
        true
    }

    async fn cancel_scope(&self, scope: &str, ctx: &CogitatorContext) {
        if let Err(error) = carry_out_cancel(&ctx.bus(), &self.self_source, scope).await {
            warn!(%error, %scope, "failed to publish cancellation");
        }
    }

    pub(super) async fn stop_turns(&self, ctx: &CogitatorContext) {
        let keys: Vec<_> = self.turns.lock().unwrap().keys().cloned().collect();
        for key in keys {
            self.cancel_channel(&key, ctx).await;
        }
        self.children.cancel_all().await;
    }
}

#[cfg(test)]
mod tests {
    use super::{ActiveTurn, ChannelState};
    use crate::status::StatusFeed;
    use rig::completion::Message;
    use std::sync::Arc;
    use tokio::{
        spawn,
        sync::{oneshot, Mutex},
        task::yield_now,
    };

    fn blocked(messages: Vec<Message>, dropped: oneshot::Sender<()>) -> ActiveTurn {
        struct DropNotice(Option<oneshot::Sender<()>>);
        impl Drop for DropNotice {
            fn drop(&mut self) {
                if let Some(tx) = self.0.take() {
                    let _ = tx.send(());
                }
            }
        }
        let guard = DropNotice(Some(dropped));
        ActiveTurn {
            continuation: None,
            compact_id: None,
            scope: "scope".into(),
            messages,
            owner: true,
            feed: StatusFeed::silent(),
            task: spawn(async move {
                let _guard = guard;
                std::future::pending::<()>().await;
            }),
        }
    }

    #[tokio::test]
    async fn successive_interrupts_keep_every_input_and_drop_only_the_current_future() {
        let mut state = ChannelState::default();
        let mut messages = vec![Message::user("first")];
        for next in ["second", "third", "fourth"] {
            let (tx, rx) = oneshot::channel();
            state.active = Some(blocked(messages, tx));
            let previous = state.interrupt().await.unwrap();
            rx.await.unwrap();
            messages = previous.messages;
            messages.push(Message::user(next));
        }
        assert_eq!(
            messages,
            ["first", "second", "third", "fourth"].map(Message::user)
        );
        assert!(state.active.is_none());
        assert!(state.interrupt().await.is_none());
    }

    #[tokio::test]
    async fn stopping_one_channel_does_not_stop_another() {
        let (tx_a, rx_a) = oneshot::channel();
        let (tx_b, mut rx_b) = oneshot::channel();
        let mut a = ChannelState {
            active: Some(blocked(vec![Message::user("a")], tx_a)),
        };
        let mut b = ChannelState {
            active: Some(blocked(vec![Message::user("b")], tx_b)),
        };
        a.interrupt().await.unwrap();
        rx_a.await.unwrap();
        assert!(rx_b.try_recv().is_err());
        b.interrupt().await.unwrap();
        rx_b.await.unwrap();
    }

    #[tokio::test]
    async fn registration_precedes_even_an_immediate_completion() {
        let state = Arc::new(Mutex::new(ChannelState::default()));
        let mut gate = state.lock().await;
        let done = state.clone();
        let task = spawn(async move {
            let mut state = done.lock().await;
            assert!(state.active.is_some());
            state.active = None;
        });
        gate.active = Some(ActiveTurn {
            continuation: None,
            compact_id: None,
            scope: "quick".into(),
            task,
            feed: StatusFeed::silent(),
            messages: vec![],
            owner: false,
        });
        yield_now().await;
        assert!(gate.active.is_some());
        drop(gate);
        yield_now().await;
        assert!(state.lock().await.active.is_none());
    }
}

#[cfg(test)]
mod integration_tests {
    use std::sync::Arc;

    use octo_core::{Blob, ChannelId, ChannelMetadata, ConnectorId, Envelope, EventKind};
    use rig::message::UserContent;

    use super::{super::UserInput, turn_key};
    use crate::{cogitator::fixture::fixture, history::HistoryStore};

    fn envelope(channel: &str, text: &str) -> Arc<Envelope> {
        Arc::new(
            Envelope::new(
                ConnectorId::new("telegram"),
                EventKind::new("chat.message"),
                text.to_string(),
            )
            .with_channel(ChannelId::new(channel)),
        )
    }

    fn input(text: &str) -> UserInput {
        UserInput {
            text: text.into(),
            images: vec![],
            seed: None,
            voice: None,
        }
    }

    #[tokio::test]
    async fn incoming_messages_are_durable_before_the_model_and_survive_cancel() {
        let (agent, ctx, history) = fixture();
        for text in ["first", "second", "third"] {
            agent
                .clone()
                .spawn_turn(envelope("chat", text), input(text), &ctx)
                .await;
        }
        let stored = history.load("chat").await;
        assert_eq!(
            stored
                .iter()
                .map(|turn| turn.content.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "third"]
        );
        let key = turn_key(&envelope("chat", ""));
        assert!(agent.cancel_channel(&key, &ctx).await);
        assert!(!agent.cancel_channel(&key, &ctx).await);
        assert_eq!(history.load("chat").await.len(), 3);
        agent
            .clone()
            .spawn_turn(envelope("chat", "fourth"), input("fourth"), &ctx)
            .await;
        let state = agent.turns.lock().unwrap()[&key].clone();
        assert_eq!(
            state.lock().await.active.as_ref().unwrap().messages.len(),
            4
        );
        agent.stop_turns(&ctx).await;
    }

    #[tokio::test]
    async fn images_and_skill_instructions_survive_an_interruption_in_memory() {
        let (agent, ctx, _) = fixture();
        let mut first = input("look");
        first.images.push(Blob::new(vec![1, 2], "image/png"));
        agent
            .clone()
            .spawn_turn(envelope("chat", "look"), first, &ctx)
            .await;
        let mut second = input("/plan");
        second.seed = Some("skill recipe".into());
        agent
            .clone()
            .spawn_turn(envelope("chat", "/plan"), second, &ctx)
            .await;
        agent
            .clone()
            .spawn_turn(envelope("chat", "also this"), input("also this"), &ctx)
            .await;
        let state = agent.turns.lock().unwrap()[&turn_key(&envelope("chat", ""))].clone();
        let locked = state.lock().await;
        let messages = &locked.active.as_ref().unwrap().messages;
        assert!(
            matches!(&messages[0], rig::completion::Message::User { content } if content.iter().any(|c| matches!(c, UserContent::Image(_))))
        );
        assert_eq!(messages[1], rig::completion::Message::user("skill recipe"));
        drop(locked);
        agent.stop_turns(&ctx).await;
    }
    #[tokio::test]
    async fn group_chatter_neither_starts_nor_interrupts_and_authors_survive_continuation() {
        let (agent, ctx, history) = fixture();
        let group = |sender: &str, called: bool, text: &str| {
            Arc::new(
                Envelope::new(
                    ConnectorId::new("telegram"),
                    EventKind::new("chat.message"),
                    text.to_string(),
                )
                .with_channel(ChannelId::new("-42"))
                .with_channel_metadata(
                    ChannelMetadata::new()
                        .with_tag("chat_type", "supergroup")
                        .with_tag("chat_id", "-42")
                        .with_tag("sender_id", sender)
                        .with_tag("addressed", called.to_string()),
                ),
            )
        };
        agent
            .clone()
            .handle(group("2", false, "ambient"), &ctx)
            .await;
        assert!(agent.turns.lock().unwrap().is_empty());
        assert!(history.load("-42").await.is_empty());
        agent
            .clone()
            .handle(group("1", true, "Albert, first"), &ctx)
            .await;
        let key = turn_key(&group("1", true, ""));
        let state = agent.turns.lock().unwrap()[&key].clone();
        let scope = state.lock().await.active.as_ref().unwrap().scope.clone();
        agent
            .clone()
            .handle(group("2", false, "ambient again"), &ctx)
            .await;
        assert_eq!(state.lock().await.active.as_ref().unwrap().scope, scope);
        agent
            .clone()
            .handle(group("2", true, "Albert, second"), &ctx)
            .await;
        let records = history.load("-42").await;
        assert_eq!(records.len(), 2);
        assert!(records[0].content.contains("\"sender_id\":\"1\""));
        assert!(records[1].content.contains("\"sender_id\":\"2\""));
        assert_eq!(
            state.lock().await.active.as_ref().unwrap().messages.len(),
            2
        );
        let scope = state.lock().await.active.as_ref().unwrap().scope.clone();
        let mut command = group("2", true, "[Reply to bot]\n/help");
        Arc::get_mut(&mut command)
            .unwrap()
            .channel_metadata
            .as_mut()
            .unwrap()
            .tags
            .insert("command_text".into(), "/help".into());
        agent.clone().handle(command, &ctx).await;
        assert_eq!(state.lock().await.active.as_ref().unwrap().scope, scope);
        agent.stop_turns(&ctx).await;
    }
    #[tokio::test]
    async fn interrupt_does_not_resurrect_a_prefix_compacted_by_another_task() {
        use crate::history::{SqliteHistory, Turn};
        use tempfile::tempdir;
        let (mut host, ctx, _) = fixture();
        let dir = tempdir().unwrap();
        let db = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        Arc::get_mut(&mut host).unwrap().history = db.clone();
        db.append("room", &[Turn::user("old original")])
            .await
            .unwrap();
        let boundary = db.context("room").await.unwrap().through_id();
        host.clone()
            .spawn_turn(
                envelope("room", "current question"),
                input("current question"),
                &ctx,
            )
            .await;
        assert!(db
            .save_compact("room", None, boundary, "durable compact")
            .await
            .unwrap());
        let incoming = envelope("room", "new question");
        host.clone()
            .spawn_turn(incoming.clone(), input("new question"), &ctx)
            .await;
        let state = host
            .turns
            .lock()
            .unwrap()
            .get(&turn_key(&incoming))
            .cloned()
            .unwrap();
        let view =
            serde_json::to_string(&state.lock().await.active.as_ref().unwrap().messages).unwrap();
        assert!(view.contains("durable compact"));
        assert!(view.contains("current question"));
        assert!(view.contains("new question"));
        assert!(!view.contains("old original"));
        host.stop_turns(&ctx).await;
    }
    #[tokio::test]
    async fn a_new_message_interrupts_subagent_wait_without_cancelling_or_consuming_the_child() {
        use crate::cogitator::provider_fixture::setup;
        use octo_core::{EventBus, Filter, SubscribeOptions};
        use serde_json::json;
        use std::time::Duration;
        use tokio::{spawn, task::yield_now, time::timeout};
        let (host, ctx, server) = setup("waiting-parent").await;
        let key = ("telegram".to_owned(), "room".to_owned());
        let (child, cancelled, done) = host
            .children
            .reserve(
                &host.config.subagents,
                "previous".into(),
                key.clone(),
                false,
                "test",
            )
            .unwrap();
        let mut scheduler = ctx
            .bus()
            .subscribe(
                Filter::by_kind("octo.scheduler.list_alarms"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let bus = ctx.bus();
        let responder = spawn(async move {
            while let Some(request) = scheduler.next().await {
                bus.publish(
                    Envelope::new(
                        ConnectorId::new("scheduler"),
                        EventKind::new("octo.scheduler.alarms"),
                        json!({"alarms":[]}),
                    )
                    .with_target(request.source.clone())
                    .with_correlation(request.id),
                )
                .await
                .unwrap();
            }
        });
        let mut replies = ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        let text = format!("WAIT_RUN: {} original task", child.id);
        host.clone()
            .spawn_turn(envelope("room", &text), input(&text), &ctx)
            .await;
        let state = host.turns.lock().unwrap().get(&key).cloned().unwrap();
        timeout(Duration::from_secs(3), async {
            loop {
                if state
                    .lock()
                    .await
                    .active
                    .as_ref()
                    .is_some_and(|a| a.feed.tool_call_count() == 1)
                {
                    break;
                }
                yield_now().await;
            }
        })
        .await
        .unwrap();
        assert!(child.result.borrow().is_null());
        timeout(
            Duration::from_millis(500),
            host.clone().spawn_turn(
                envelope("room", "new direction"),
                input("new direction"),
                &ctx,
            ),
        )
        .await
        .unwrap();
        let reply = timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reply.payload_as::<String>().unwrap(),
            "answer from waiting-parent"
        );
        assert!(
            !*cancelled.borrow(),
            "ordinary interrupt must not cancel the child"
        );
        done.send_replace(json!({"outcome":{"status":"completed","answer":"late child report"}}));
        // A detached old tool future must neither send a stale answer nor mark
        // this late report as collected by the interrupted parent.
        assert!(timeout(Duration::from_millis(100), replies.next())
            .await
            .is_err());
        assert!(host.children.context(&key, false).contains(&child.id));
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let resumed = requests[1]["messages"].to_string();
        assert!(resumed.contains("original task") && resumed.contains("new direction"));
        assert!(resumed.contains("UNKNOWN"));
        drop(requests);
        responder.abort();
        host.stop_turns(&ctx).await;
    }
    #[tokio::test]
    async fn delivered_child_report_is_acknowledged_after_the_parent_records_it() {
        use crate::{cogitator::provider_fixture::setup, status::StatusFeed};
        use rig::completion::Message;
        use serde_json::json;
        use std::time::Duration;
        use tokio::time::timeout;
        let (host, ctx, _server) = setup("waiting-parent").await;
        let key = ("telegram".to_owned(), "room".to_owned());
        let (child, _, done) = host
            .children
            .reserve(
                &host.config.subagents,
                "previous".into(),
                key.clone(),
                false,
                "test",
            )
            .unwrap();
        done.send_replace(
            json!({"outcome":{"status":"completed","answer":"verified child report"}}),
        );
        let feed = StatusFeed::silent();
        timeout(
            Duration::from_secs(3),
            host.run_agent(
                &ctx,
                "room",
                "test",
                Message::user(format!("WAIT_RUN: {} original task", child.id)),
                vec![],
                Some(ConnectorId::new("telegram")),
                false,
                feed.clone(),
                Some("parent"),
            ),
        )
        .await
        .unwrap();
        assert!(host.children.context(&key, false).is_empty());
        assert!(serde_json::to_string(&feed.snapshot())
            .unwrap()
            .contains("verified child report"));
    }
}
