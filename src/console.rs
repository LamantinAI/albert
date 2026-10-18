//! `ConsoleConnector` — a minimal bidirectional connector over stdin/stdout, the
//! Telegram stand-in. Each stdin line becomes a `chat.message`; every `chat.reply`
//! targeted at us is printed. Same envelope shapes as the real Telegram connector.

use std::{sync::Arc, time::Duration};

use async_trait::async_trait;
use octo_approval::{Access, Authority, Origin};
use octo_core::{
    Blob, ChannelId, Connector, ConnectorCapabilities, ConnectorContext, ConnectorId, Envelope,
    EventKind, Filter, OctoError, OctoResult, ReplyChannel, SubscribeOptions,
};
use serde_json::{json, Value};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    select,
};
use tracing::warn;

const CHANNEL: &str = "stdin";

pub struct ConsoleConnector {
    id: ConnectorId,
    capabilities: ConnectorCapabilities,
    authority: Option<(Authority, Duration)>,
}

impl ConsoleConnector {
    pub fn new(id: impl Into<String>) -> Arc<Self> {
        let capabilities = ConnectorCapabilities::bidirectional()
            .with_emit_kinds([EventKind::from_static("chat.message")])
            .with_accept_kinds([EventKind::from_static("chat.reply")]);
        Arc::new(Self {
            id: ConnectorId::new(id),
            capabilities,
            authority: None,
        })
    }
    pub fn with_authority(
        mut self: Arc<Self>,
        authority: Authority,
        timeout: Duration,
    ) -> Arc<Self> {
        Arc::get_mut(&mut self)
            .expect("configure before sharing")
            .authority = Some((authority, timeout));
        self
    }
}

#[async_trait]
impl Connector for ConsoleConnector {
    fn id(&self) -> &ConnectorId {
        &self.id
    }

    fn capabilities(&self) -> &ConnectorCapabilities {
        &self.capabilities
    }

    async fn run(self: Arc<Self>, ctx: ConnectorContext) -> OctoResult<()> {
        let mut replies = ctx
            .subscribe(
                Filter::by_target(self.id.clone()),
                SubscribeOptions::default(),
            )
            .await?;

        let mut lines = BufReader::new(stdin()).lines();
        println!("albert ready — type a message (Ctrl-D to quit):");

        loop {
            select! {
                line = lines.next_line() => match line {
                    Ok(Some(text)) => {
                        if text.trim().is_empty() {
                            continue;
                        }
                        let mut msg = Envelope::new(
                            self.id.clone(),
                            EventKind::from_static("chat.message"),
                            text,
                        )
                        .with_channel(ChannelId::new(CHANNEL))
                        .with_reply_to(ReplyChannel::new(ChannelId::new(CHANNEL)));
                        if let Some((authority, timeout)) = &self.authority {
                            let ingress = authority.issue(Origin {
                                principal: "console:local".into(), connector: self.id.to_string(),
                                conversation: CHANNEL.into(), private_destination: CHANNEL.into(), may_approve: true,
                            }, Access::owner());
                            let text = msg.payload_as::<String>().expect("console text");
                            let parts: Vec<_> = text.split_whitespace().collect();
                            if matches!(parts.first().copied(), Some("/sqlapprove" | "/sqldeny")) {
                                if parts.len() != 3 {
                                    println!("Use /sqlapprove <connector> <operation_id> or /sqldeny <connector> <operation_id>.");
                                    continue;
                                }
                                let request = Envelope::new(self.id.clone(), EventKind::new("sql.approval.decide"),
                                    json!({"operation_id":parts[2],"approve":parts[0]=="/sqlapprove"}))
                                    .with_target(ConnectorId::new(parts[1]));
                                let proof = authority.bind_decision(&ingress, &request)
                                    .map_err(OctoError::Connector)?;
                                match ctx.publish_and_await_response(request.with_context(proof), *timeout).await {
                                    Ok(reply) => println!("database operation: {}",reply.payload_as::<Value>()
                                        .and_then(|v|v.get("status").or(v.get("error")))
                                        .unwrap_or(&Value::Null)),
                                    Err(_) => println!("Decision sent; outcome unknown. Check operation status before retrying."),
                                }
                                continue;
                            }
                            msg = msg.with_context(ingress);
                        }
                        ctx.publish(msg).await?;
                    }
                    Ok(None) => {
                        ctx.shutdown.cancel();
                        return Ok(());
                    }
                    Err(e) => {
                        warn!(error = %e, "console stdin error; shutting down");
                        ctx.shutdown.cancel();
                        return Ok(());
                    }
                },
                reply = replies.next() => match reply {
                    Some(env) => {
                        if let Some(blob) = env.payload_as::<Blob>() {
                            println!("[media: {}, {} bytes]", blob.content_type(), blob.len());
                        } else if let Some(text) = env.payload_as::<String>() {
                            println!("bot: {text}");
                        }
                    }
                    None => return Ok(()),
                },
                _ = ctx.shutdown.cancelled() => return Ok(()),
            }
        }
    }
}
