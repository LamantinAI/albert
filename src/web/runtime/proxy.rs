use std::sync::Arc;

use async_trait::async_trait;
use octo_core::{
    Cogitator, CogitatorContext, ConnectorId, Envelope, EventKind, Filter, OctoResult, Subscription,
};
use tokio::{select, task::JoinSet};

use super::AgentRuntime;

pub(crate) struct RuntimeCogitator(pub Arc<AgentRuntime>);

#[async_trait]
impl Cogitator for RuntimeCogitator {
    fn id(&self) -> &str {
        "albert"
    }
    fn filter(&self) -> Filter {
        Filter::by_kind("chat.message")
            .with_kind("alarm.fired")
            .with_kind("albert.session.closed")
    }

    async fn run(
        self: Arc<Self>,
        context: CogitatorContext,
        mut events: Subscription,
    ) -> OctoResult<()> {
        *self.0.context.write().unwrap() = Some(context.clone());
        let _ = self.0.activate().await;
        let mut maintenance = JoinSet::new();
        loop {
            select! {
                event = events.next() => match event {
                    Some(event) => {
                        if let Some(agent) = self.0.current() {
                            if matches!(event.kind.as_str(), "alarm.fired" | "albert.session.closed") {
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
        maintenance.shutdown().await;
        Ok(())
    }
}
