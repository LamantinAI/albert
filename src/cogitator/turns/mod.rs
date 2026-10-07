//! Albert's interruption policy. A channel transition is serialized with reply
//! publication; only the model/tool future is aborted, never accepted input.

use std::sync::{atomic::Ordering, Arc};

use octo_core::{CogitatorContext, Envelope};
use octo_rig::carry_out_cancel;
use rig::completion::Message;
use tokio::{spawn, sync::Mutex, task::JoinHandle};
use tracing::{info, warn};

use super::{
    action_context, channel_of, incoming_context, now_rfc3339, with_action_log, AlbertCogitator,
    UserInput,
};
use crate::{
    acl::is_owner,
    history::{assistant_turn, journal_messages, to_messages, tool_trace, Turn},
    status::StatusFeed,
};

#[derive(Default)]
pub(super) struct ChannelState {
    active: Option<ActiveTurn>,
}

struct ActiveTurn {
    scope: String,
    task: JoinHandle<()>,
    feed: StatusFeed,
    /// Full working context, including images and skill instructions, survives
    /// every interruption even when the persisted rolling history trims old turns.
    messages: Vec<Message>,
    owner: bool,
}

struct Interrupted {
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
    pub(super) async fn spawn_turn(
        self: Arc<Self>,
        incoming: Arc<Envelope>,
        input: UserInput,
        ctx: &CogitatorContext,
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
        let mut records = Vec::new();
        let (mut messages, owner) = if let Some(previous) = interrupted {
            self.cancel_scope(&previous.scope, ctx).await;
            records.extend(tool_trace(&previous.checkpoint));
            // A trusted user's work must not gain owner tools merely because the
            // next participant is the owner. Re-evaluate at the next fresh turn.
            (previous.messages, previous.owner && is_owner(&incoming))
        } else {
            (
                to_messages(&self.history.load(&channel).await),
                is_owner(&incoming),
            )
        };
        records.push(Turn::user(input.transcript_with_source(&incoming)));
        if let Err(error) = self.history.append(&channel, &records).await {
            warn!(%error, %channel, "cannot persist incoming message; refusing to start tools");
            self.emit_reply(
                &incoming,
                "I couldn't save your message, so I haven't started the work. Please try again."
                    .into(),
                ctx,
            )
            .await;
            return;
        }
        info!(source = %incoming.source, %channel, "← {}", input.transcript());
        messages.push(input.prompt_with_source(&incoming));
        let scope = format!(
            "{}/{}-{}",
            self.id,
            incoming.id,
            self.turn_seq.fetch_add(1, Ordering::Relaxed)
        );
        let feed = self.feed(ctx, incoming.source.clone(), incoming.channel.clone());
        let me = self.clone();
        let owned_ctx = ctx.clone();
        let owned_state = state.clone();
        let owned_scope = scope.clone();
        let owned_feed = feed.clone();
        let mut history = messages.clone();
        let prompt = history.pop().expect("new input was appended");
        let voice = input.voice;
        let task = spawn(async move {
            me.run_turn(
                incoming,
                voice,
                prompt,
                history,
                owner,
                &owned_ctx,
                &owned_scope,
                owned_feed,
                owned_state,
            )
            .await;
        });
        // Insert before releasing the gate; a fast completion cannot clear its
        // slot before registration (the old spawn/insert race).
        locked.active = Some(ActiveTurn {
            scope,
            task,
            feed,
            messages,
            owner,
        });
    }

    pub(super) async fn cancel_channel(
        &self,
        key: &(String, String),
        ctx: &CogitatorContext,
    ) -> bool {
        let state = self.turns.lock().unwrap().get(key).cloned();
        let Some(state) = state else { return false };
        let mut locked = state.lock().await;
        let Some(previous) = locked.interrupt().await else {
            return false;
        };
        self.cancel_scope(&previous.scope, ctx).await;
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

    async fn run_turn(
        self: &Arc<Self>,
        incoming: Arc<Envelope>,
        voice: Option<String>,
        mut prompt: Message,
        mut history: Vec<Message>,
        owner: bool,
        ctx: &CogitatorContext,
        scope: &str,
        feed: StatusFeed,
        state: Arc<Mutex<ChannelState>>,
    ) {
        let channel = channel_of(&incoming);
        self.emit_typing(incoming.source.clone(), incoming.channel.clone(), ctx)
            .await;
        let active = self.active_reminders(ctx).await;
        let pad = self.scratchpad.render(&channel);
        let stored = self.history.load(&channel).await;
        let preamble = format!(
            "{}\n\n{}\n\nCurrent time: {}\n\n{}\n\n{}\n\n{}{}",
            self.prompt.base(),
            incoming_context(&incoming, &channel),
            now_rfc3339(&self.config.timezone),
            active,
            pad,
            self.skills.catalog(),
            action_context(&stored)
        );
        let hearing_error = if let Some(path) = voice {
            match self.hear(&incoming, &path, ctx, scope, &feed).await {
                Ok(text) => {
                    history.push(prompt.clone());
                    history.extend(journal_messages(&feed.snapshot()));
                    prompt = Message::user(text);
                    None
                }
                Err(error) => Some(error),
            }
        } else {
            None
        };
        let (answer, restart) = if let Some(error) = hearing_error {
            (error, None)
        } else {
            self.run_agent(
                ctx,
                &channel,
                &preamble,
                prompt,
                history,
                Some(incoming.source.clone()),
                owner,
                feed.clone(),
                Some(scope),
            )
            .await
        };

        // Reply commit and accepting a newer input cannot interleave. If the new
        // input won, this task has been aborted while waiting for the gate.
        let mut locked = state.lock().await;
        if !locked
            .active
            .as_ref()
            .is_some_and(|active| active.scope == scope)
        {
            return;
        }
        let checkpoint = feed.checkpoint();
        let mut records: Vec<_> = tool_trace(&checkpoint).into_iter().collect();
        records.push(assistant_turn(with_action_log(
            &answer,
            &feed.drain_actions(),
        )));
        if let Err(error) = self.history.append(&channel, &records).await {
            warn!(%error, "failed to save completed turn");
        }
        info!("→ {answer}");
        self.emit_reply(&incoming, answer, ctx).await;
        locked.active = None;
        drop(locked);
        if let Some(target) = restart {
            self.apply_restart(target, ctx).await;
        }
    }

    pub(super) async fn stop_turns(&self, ctx: &CogitatorContext) {
        let keys: Vec<_> = self.turns.lock().unwrap().keys().cloned().collect();
        for key in keys {
            self.cancel_channel(&key, ctx).await;
        }
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
}
