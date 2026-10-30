use std::sync::{atomic::Ordering, Arc};

use chrono::Utc;
use octo_core::{ChannelId, CogitatorContext, ConnectorId};
use tokio::{
    spawn,
    sync::{watch::channel, Mutex},
};
use tracing::{debug, warn};

use super::{execution::MessageExecution, routes::RuntimeRoutes, MessageTask};
use crate::{
    cogitator::{turns::ChannelState, AlbertCogitator},
    context::DiagnosticScope,
    messaging::{
        contract::{ExecutionLocation, TrustedSource, WorkRef},
        store::{DeliveryFailure, WorkTerminal},
        MessagingError,
    },
};

impl AlbertCogitator {
    pub(in crate::cogitator) async fn messaging_tick(self: &Arc<Self>, ctx: &CogitatorContext) {
        let (Some(runtime), Some(service)) = (&self.messaging, &self.session_service) else {
            return;
        };
        if !runtime.recovered.swap(true, Ordering::SeqCst) {
            match runtime.store.recover(runtime.settings.batch_size).await {
                Ok(items) => {
                    for item in items {
                        debug!(message_id=%item.message_id, action=?item.action, "message recovery classification");
                    }
                }
                Err(error) => {
                    runtime.recovered.store(false, Ordering::SeqCst);
                    warn!(%error, "message work recovery failed");
                    return;
                }
            }
        }
        // Reap bounded finished supervisors even when their session receives no
        // further messages. This also reconciles an unexpected task panic.
        let states: Vec<_> = self.turns.lock().unwrap().values().cloned().collect();
        let mut reaped = 0;
        for state in states {
            if reaped >= runtime.settings.batch_size {
                break;
            }
            let Ok(mut gate) = state.try_lock() else {
                continue;
            };
            if gate.message.as_ref().is_some_and(MessageTask::is_finished) {
                if let Some(task) = gate.message.take() {
                    task.stop(self, ctx, WorkTerminal::Interrupted).await;
                    reaped += 1;
                }
            }
        }
        let now = Utc::now().timestamp_millis();
        let after = runtime.cursor.lock().unwrap().clone();
        let page = match runtime
            .store
            .scheduler_page(after.as_deref(), runtime.settings.batch_size, now)
            .await
        {
            Ok(page) => page,
            Err(error) => {
                warn!(%error, "message queue enumeration failed");
                return;
            }
        };
        *runtime.cursor.lock().unwrap() = page
            .last()
            .map(|entry| entry.envelope.id.as_str().to_owned());
        let guard = RuntimeRoutes::new(ctx);
        for entry in page {
            let id = entry.envelope.id.clone();
            let target = if !entry.has_copy {
                match runtime.store.dispatch(&id, &guard, now).await {
                    Ok(receipt) => receipt.target.session,
                    Err(error) => {
                        let failure = match error {
                            MessagingError::Denied => DeliveryFailure::Revoked,
                            MessagingError::SaveRequired => DeliveryFailure::SaveRequired,
                            MessagingError::Limit => DeliveryFailure::Limit,
                            _ => DeliveryFailure::Unavailable,
                        };
                        let _ = runtime.store.fail_delivery(&id, failure).await;
                        continue;
                    }
                }
            } else if let Some(recipient) = entry.recipient {
                recipient
            } else {
                continue;
            };
            if !entry.envelope.intent.schedules_work() {
                continue;
            }
            let active: usize = self
                .turns
                .lock()
                .unwrap()
                .values()
                .filter_map(|state| state.try_lock().ok())
                .filter(|state| {
                    state
                        .message
                        .as_ref()
                        .is_some_and(|task| !task.is_finished())
                })
                .count();
            if active >= runtime.settings.max_concurrent {
                continue;
            }
            let key = ("session".into(), format!("session/{target}"));
            let state = self
                .turns
                .lock()
                .unwrap()
                .entry(key)
                .or_insert_with(|| Arc::new(Mutex::new(ChannelState::default())))
                .clone();
            let Ok(mut gate) = state.try_lock() else {
                continue;
            };
            if !gate.is_idle() {
                continue;
            }
            let context = match service
                .messaging_context(
                    &entry.envelope.actor,
                    &target,
                    &entry.envelope.return_address,
                )
                .await
            {
                Ok(context) => context,
                Err(_) => {
                    let _ = runtime
                        .store
                        .fail_delivery(&id, DeliveryFailure::Unavailable)
                        .await;
                    continue;
                }
            };
            let scope = format!("{}/message/{}", self.id, id.as_str());
            let source = match TrustedSource::current(
                &context,
                WorkRef::Turn(scope.clone()),
                ExecutionLocation::default(),
            ) {
                Ok(source) => source,
                Err(_) => continue,
            };
            let work = match runtime.store.claim_work(&source, &id, &guard, now).await {
                Ok(work) => work,
                Err(_) => {
                    let _ = runtime
                        .store
                        .finish_work(&source, &id, WorkTerminal::Revoked)
                        .await;
                    continue;
                }
            };
            let (cancel, cancelled) = channel(WorkTerminal::Interrupted);
            let me = self.clone();
            let runtime = runtime.clone();
            let ctx = ctx.clone();
            let owned_scope = scope.clone();
            let task_source = source.clone();
            let feed = self
                .feed(
                    &ctx,
                    ConnectorId::new(source.context().reply_route().connector()),
                    Some(ChannelId::new(source.context().reply_route().channel())),
                )
                .with_session(Some(source.context().session_id().as_str()))
                .with_session_context(Some(source.context().clone()))
                .initially_detached()
                .with_diagnostics(Some(DiagnosticScope {
                    lane: "message",
                    session_id: Some(source.context().session_id().as_str().into()),
                    work_id: Some(id.as_str().into()),
                    attempt_id: scope.clone(),
                    run_id: None,
                }));
            let owned_feed = feed.clone();
            let task = spawn(async move {
                me.execute_message(MessageExecution {
                    runtime,
                    ctx,
                    source,
                    work,
                    scope: owned_scope,
                    cancelled,
                    feed: owned_feed,
                })
                .await;
            });
            gate.message = Some(MessageTask {
                id,
                scope,
                cancel,
                task,
                feed,
                source: task_source,
            });
        }
        self.message_failure_notices(runtime, ctx).await;
        self.deliver_reports(runtime, ctx).await;
        let previous = runtime.cleaned_at.load(Ordering::Relaxed);
        if now - previous >= 60_000
            && runtime
                .cleaned_at
                .compare_exchange(previous, now, Ordering::SeqCst, Ordering::Relaxed)
                .is_ok()
        {
            let before = now.saturating_sub(runtime.settings.retention_secs as i64 * 1000);
            let _ = runtime
                .store
                .cleanup_terminal(before, now, runtime.settings.batch_size)
                .await;
        }
    }
}
