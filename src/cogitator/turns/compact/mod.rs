use std::sync::{atomic::Ordering, Arc};

use octo_core::{CogitatorContext, Envelope};
use rig::completion::Message;
use tokio::{spawn, sync::Mutex};

use super::{turn_key, ActiveTurn, ChannelState};
use crate::{
    cogitator::{
        channel_of,
        compaction::{CompactOptions, CompactOutcome},
        AlbertCogitator,
    },
    history::tool_trace,
    status::StatusFeed,
};

impl AlbertCogitator {
    /// /compact uses the same per-conversation gate as ordinary turns, and can
    /// itself be interrupted. Children are deliberately left running.
    pub(crate) async fn spawn_compact(
        self: Arc<Self>,
        incoming: Arc<Envelope>,
        ctx: &CogitatorContext,
        options: CompactOptions,
    ) {
        let channel = channel_of(&incoming);
        let state = self
            .turns
            .lock()
            .unwrap()
            .entry(turn_key(&incoming))
            .or_insert_with(|| Arc::new(Mutex::new(ChannelState::default())))
            .clone();
        let mut locked = state.lock().await;
        let interrupted = locked.interrupt().await;
        let continuation = interrupted.as_ref().and_then(|p| p.continuation.clone());
        let owner = interrupted.as_ref().is_none_or(|p| p.owner);
        if let Some(previous) = interrupted {
            self.cancel_scope(&previous.scope, ctx).await;
            if let Some(record) = tool_trace(&previous.checkpoint) {
                if self.history.append(&channel, &[record]).await.is_err() {
                    self.emit_reply(
                        &incoming,
                        "Could not save interrupted work; compaction was not started.".into(),
                        ctx,
                    )
                    .await;
                    return;
                }
            }
        }
        let (messages, compact_id) = match self.visible_snapshot(&channel).await {
            Ok(messages) => messages,
            Err(error) => {
                self.emit_reply(&incoming, error, ctx).await;
                return;
            }
        };
        let scope = format!(
            "{}/compact-{}",
            self.id,
            self.turn_seq.fetch_add(1, Ordering::Relaxed)
        );
        let me = self.clone();
        let owned_ctx = ctx.clone();
        let owned_state = state.clone();
        let owned_scope = scope.clone();
        let progress = StatusFeed::new(
            ctx.bus(),
            self.self_source.clone(),
            incoming.source.clone(),
            incoming.channel.clone(),
        );
        let resume = continuation.clone();
        let task = spawn(async move {
            me.emit_typing(
                incoming.source.clone(),
                incoming.channel.clone(),
                &owned_ctx,
            )
            .await;
            let outcome = me
                .compact_with_options(&channel, true, false, &progress, &options)
                .await;
            if outcome.is_ok() {
                if let Some(original) = resume {
                    me.resume_after_compact(
                        original,
                        owner,
                        &owned_ctx,
                        &owned_scope,
                        owned_state,
                        &progress,
                    )
                    .await;
                    return;
                }
            }
            let answer = match outcome {
                Ok(CompactOutcome::Saved) => {
                    "Conversation compacted. Original messages remain in history storage.".into()
                }
                Ok(CompactOutcome::Unchanged) => "No conversation messages to compact.".into(),
                Ok(CompactOutcome::DryRun { before, after }) => format!("Compaction dry run complete (~{before} -> ~{after} tokens, estimated). The current compact and its message boundary were not changed."),
                Err(error) => format!("Could not compact conversation history: {error}"),
            };
            me.finish_context_reply(&incoming, answer, &owned_ctx, &owned_scope, &owned_state)
                .await;
        });
        locked.active = Some(ActiveTurn {
            continuation,
            compact_id,
            scope,
            task,
            messages,
            owner,
            feed: StatusFeed::silent(),
        });
    }

    /// Stay in the same registered task/scope: cancellation or a newer input
    /// aborts this transition too, so no detached continuation can resurrect work.
    async fn resume_after_compact(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        owner: bool,
        ctx: &CogitatorContext,
        scope: &str,
        state: Arc<Mutex<ChannelState>>,
        progress: &StatusFeed,
    ) {
        let (history, revision) = match self.visible_snapshot(&channel_of(&incoming)).await {
            Ok(snapshot) => snapshot,
            Err(error) => {
                self.finish_context_reply(&incoming, error, ctx, scope, &state)
                    .await;
                return;
            }
        };
        let prompt = Message::user("[Runtime continuation after manual compaction] Continue the interrupted task from the conversation context. Preserve completed actions; verify UNKNOWN outcomes before repeating any action. Check existing subagents and collect their results instead of launching replacements. This is a continuation, not a new user request.");
        let feed = self.feed(ctx, incoming.source.clone(), incoming.channel.clone());
        {
            let mut locked = state.lock().await;
            let Some(active) = locked.active.as_mut().filter(|a| a.scope == scope) else {
                return;
            };
            active.compact_id = revision;
            active.messages = history.clone();
            active.messages.push(prompt.clone());
            active.feed = feed.clone();
        }
        progress
            .progress("Compaction finished. Continuing the interrupted task...")
            .await;
        self.run_turn(
            incoming, None, prompt, history, owner, ctx, scope, feed, state,
        )
        .await;
    }

    pub(super) async fn finish_context_reply(
        &self,
        incoming: &Envelope,
        answer: String,
        ctx: &CogitatorContext,
        scope: &str,
        state: &Arc<Mutex<ChannelState>>,
    ) {
        let mut locked = state.lock().await;
        if locked
            .active
            .as_ref()
            .is_some_and(|active| active.scope == scope)
        {
            self.emit_reply(incoming, answer, ctx).await;
            locked.active = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_core::{
        ChannelId, CogitatorContext, ConnectorId, Envelope, EventBus, EventKind, Filter,
        SubscribeOptions,
    };
    use serde_json::json;
    use tempfile::{tempdir, TempDir};
    use tokio::{spawn, task::JoinHandle, time::timeout};

    use crate::{
        cogitator::{
            compaction::CompactOptions,
            provider_fixture::{setup, Server},
            AlbertCogitator, UserInput,
        },
        history::{HistoryStore, SqliteHistory, Turn},
    };

    use super::super::turn_key;

    fn envelope(text: &str) -> Arc<Envelope> {
        Arc::new(
            Envelope::new(
                ConnectorId::new("telegram"),
                EventKind::new("chat.message"),
                text.to_owned(),
            )
            .with_channel(ChannelId::new("room")),
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

    struct Scheduler(JoinHandle<()>);
    impl Drop for Scheduler {
        fn drop(&mut self) {
            self.0.abort();
        }
    }

    async fn fixture(
        model: &str,
    ) -> (
        Arc<AlbertCogitator>,
        CogitatorContext,
        Server,
        TempDir,
        Scheduler,
    ) {
        let (mut host, ctx, server) = setup(model).await;
        let dir = tempdir().unwrap();
        let db = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        db.append("room", &[Turn::user("old payload ".repeat(100))])
            .await
            .unwrap();
        Arc::get_mut(&mut host).unwrap().history = db;
        let mut requests = ctx
            .bus()
            .subscribe(
                Filter::by_kind("octo.scheduler.list_alarms"),
                SubscribeOptions::default(),
            )
            .await
            .unwrap();
        let bus = ctx.bus();
        let scheduler = Scheduler(spawn(async move {
            while let Some(request) = requests.next().await {
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
        }));
        (host, ctx, server, dir, scheduler)
    }

    async fn wait_calls(server: &Server, count: usize) {
        timeout(Duration::from_secs(3), async {
            loop {
                if server.requests.lock().unwrap().len() >= count {
                    break;
                }
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
    }

    #[tokio::test]
    async fn compact_resumes_waiting_parent_from_sql_and_keeps_child_alive() {
        resume_waiting_parent(false).await;
    }

    #[tokio::test]
    async fn dry_run_resumes_waiting_parent_without_saving_compact() {
        resume_waiting_parent(true).await;
    }

    async fn resume_waiting_parent(dry_run: bool) {
        let (host, ctx, server, _dir, _scheduler) = fixture("waiting-parent").await;
        let key = turn_key(&envelope(""));
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
        let mut replies = ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        let task = format!("WAIT_RUN: {} original task", child.id);
        host.clone()
            .spawn_turn(envelope(&task), input(&task), &ctx)
            .await;
        let state = host.turns.lock().unwrap()[&key].clone();
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
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        host.clone()
            .spawn_compact(
                envelope("/compact"),
                &ctx,
                CompactOptions {
                    dry_run,
                    ..Default::default()
                },
            )
            .await;
        let reply = timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reply.payload_as::<String>().unwrap(),
            "answer from waiting-parent"
        );
        assert!(!*cancelled.borrow());
        done.send_replace(json!({"outcome":{"status":"completed","answer":"late result"}}));
        assert!(host.children.context(&key, false).contains(&child.id));
        let window = host.history.context("room").await.unwrap();
        assert_eq!(window.compact.is_some(), !dry_run);
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 3);
        let compact = requests[1]["messages"].to_string();
        assert!(compact.contains("UNKNOWN") && compact.contains(&child.id));
        let resumed = requests[2]["messages"].to_string();
        assert!(resumed.contains("Runtime continuation") && resumed.contains(&child.id));
        assert_eq!(resumed.contains("old payload"), dry_run);
        let tools = requests[2]["tools"].to_string();
        assert!(
            !tools.contains("model_select"),
            "owner-only compact must not elevate original non-owner task"
        );
        drop(requests);
        assert!(timeout(Duration::from_millis(50), replies.next())
            .await
            .is_err());
        host.stop_turns(&ctx).await;
    }

    #[tokio::test]
    async fn idle_compact_does_not_start_a_task() {
        let (host, ctx, server, _dir, _scheduler) = fixture("healthy").await;
        let mut replies = ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        host.clone()
            .spawn_compact(envelope("/compact"), &ctx, CompactOptions::default())
            .await;
        let reply = timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        assert!(reply
            .payload_as::<String>()
            .unwrap()
            .starts_with("Conversation compacted."));
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(host.turns.lock().unwrap()[&turn_key(&envelope(""))]
            .lock()
            .await
            .active
            .is_none());
        host.stop_turns(&ctx).await;
    }

    #[tokio::test]
    async fn cancel_during_compact_prevents_continuation() {
        let (host, ctx, server, _dir, _scheduler) = fixture("compact-delayed").await;
        // Register an active task before its model call can start.
        host.clone()
            .spawn_turn(envelope("original task"), input("original task"), &ctx)
            .await;
        host.clone()
            .spawn_compact(envelope("/compact"), &ctx, CompactOptions::default())
            .await;
        wait_calls(&server, 1).await;
        assert!(host.cancel_channel(&turn_key(&envelope("")), &ctx).await);
        tokio::time::sleep(Duration::from_millis(200)).await;
        assert_eq!(server.requests.lock().unwrap().len(), 1);
        assert!(host
            .history
            .context("room")
            .await
            .unwrap()
            .compact
            .is_none());
        host.stop_turns(&ctx).await;
    }

    #[tokio::test]
    async fn new_message_during_compact_supersedes_automatic_continuation() {
        let (host, ctx, server, _dir, _scheduler) = fixture("compact-delayed").await;
        let mut replies = ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        host.clone()
            .spawn_turn(envelope("original task"), input("original task"), &ctx)
            .await;
        host.clone()
            .spawn_compact(envelope("/compact"), &ctx, CompactOptions::default())
            .await;
        wait_calls(&server, 1).await;
        host.clone()
            .spawn_turn(envelope("new direction"), input("new direction"), &ctx)
            .await;
        timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        tokio::time::sleep(Duration::from_millis(200)).await;
        let requests = server.requests.lock().unwrap();
        assert_eq!(requests.len(), 2);
        let resumed = requests[1]["messages"].to_string();
        assert!(resumed.contains("new direction") && resumed.contains("original task"));
        assert!(!resumed.contains("Runtime continuation"));
        drop(requests);
        assert!(host
            .history
            .context("room")
            .await
            .unwrap()
            .compact
            .is_none());
        host.stop_turns(&ctx).await;
    }
    #[tokio::test]
    async fn repeated_compact_keeps_the_task_and_resumes_only_once() {
        let (host, ctx, server, _dir, _scheduler) = fixture("compact-delayed").await;
        let mut replies = ctx
            .bus()
            .subscribe(Filter::by_kind("chat.reply"), SubscribeOptions::default())
            .await
            .unwrap();
        host.clone()
            .spawn_turn(envelope("original task"), input("original task"), &ctx)
            .await;
        host.clone()
            .spawn_compact(envelope("/compact"), &ctx, CompactOptions::default())
            .await;
        wait_calls(&server, 1).await;
        host.clone()
            .spawn_compact(envelope("/compact"), &ctx, CompactOptions::default())
            .await;
        let reply = timeout(Duration::from_secs(3), replies.next())
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            reply.payload_as::<String>().unwrap(),
            "answer from compact-delayed"
        );
        assert_eq!(server.requests.lock().unwrap().len(), 3);
        let records = host.history.load("room").await;
        assert_eq!(
            records
                .iter()
                .filter(|r| r.content == "original task")
                .count(),
            1
        );
        assert!(host
            .history
            .context("room")
            .await
            .unwrap()
            .compact
            .is_some());
        assert!(timeout(Duration::from_millis(200), replies.next())
            .await
            .is_err());
        host.stop_turns(&ctx).await;
    }
}
