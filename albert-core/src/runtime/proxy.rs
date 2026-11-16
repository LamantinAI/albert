use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use octo_core::{
    Cogitator, CogitatorContext, ConnectorId, Envelope, EventKind, Filter, OctoResult, Subscription,
};
use tokio::{select, task::JoinSet, time::interval, time::MissedTickBehavior};

use super::AgentRuntime;

pub struct RuntimeCogitator(pub Arc<AgentRuntime>);

#[async_trait]
impl Cogitator for RuntimeCogitator {
    fn id(&self) -> &str {
        "albert"
    }
    fn filter(&self) -> Filter {
        Filter::by_kind("chat.message")
            .with_kind("alarm.fired")
            .with_kind("albert.session.closed")
            .with_kind("albert.presentation.detached")
            .with_kind("albert.presentation.opened")
    }

    async fn run(
        self: Arc<Self>,
        context: CogitatorContext,
        mut events: Subscription,
    ) -> OctoResult<()> {
        *self.0.context.write().unwrap() = Some(context.clone());
        let _ = self.0.activate().await;
        self.0.recover_session_deletions().await;
        let mut maintenance = JoinSet::new();
        let mut erasures = JoinSet::new();
        let mut erasure_tick = interval(Duration::from_secs(5));
        erasure_tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        let mut messaging = interval(Duration::from_millis(
            self.0.base.sessions.messaging.poll_ms,
        ));
        messaging.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            select! {
                _ = erasure_tick.tick(), if erasures.is_empty() => {
                    let runtime = self.0.clone();
                    erasures.spawn(async move { runtime.recover_session_deletions().await; });
                },
                _ = erasures.join_next(), if !erasures.is_empty() => {},
                _ = messaging.tick() => { if let Some(agent)=self.0.current() { agent.panel_maintenance(&context).await; } },
                event = events.next() => match event {
                    Some(event) => {
                        if let Some(agent) = self.0.current() {
                            if matches!(event.kind.as_str(), "alarm.fired" | "albert.session.closed" | "albert.presentation.detached" | "albert.presentation.opened") {
                                let ctx = context.clone();
                                maintenance.spawn(async move { agent.panel_handle(event, &ctx).await; });
                            } else { agent.panel_handle(event, &context).await; }
                        } else if event.kind.as_str() == "chat.message" {
                            let mut reply = Envelope::new(ConnectorId::new("cogitator/albert"), EventKind::new("chat.reply"),
                                "Albert is not configured yet. Open the web panel and connect a model.".to_owned())
                                .with_target(event.source.clone()).with_correlation(event.id);
                            reply.channel = event.channel.clone();
                            let _ = context.publish(reply).await;
                        }
                    },
                    None => break,
                },
                _ = maintenance.join_next(), if !maintenance.is_empty() => {},
                _ = context.shutdown.cancelled() => break,
            }
        }
        if let Some(agent) = self.0.current() {
            agent.panel_stop(&context).await;
        }
        self.0.mcp.close().await;
        maintenance.shutdown().await;
        erasures.shutdown().await;
        Ok(())
    }
}
