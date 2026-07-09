//! `ConsoleConnector` — a minimal bidirectional connector over stdin/stdout, the
//! Telegram stand-in. Each stdin line becomes a `chat.message`; every `chat.reply`
//! targeted at us is printed. Same envelope shapes as the real Telegram connector.

use std::sync::Arc;

use async_trait::async_trait;
use octo_core::{
    Blob, ChannelId, Connector, ConnectorCapabilities, ConnectorContext, ConnectorId, Envelope,
    EventKind, Filter, OctoResult, ReplyChannel, SubscribeOptions,
};
use tokio::{
    io::{stdin, AsyncBufReadExt, BufReader},
    select,
};

const CHANNEL: &str = "stdin";

pub struct ConsoleConnector {
    id: ConnectorId,
    capabilities: ConnectorCapabilities,
}

impl ConsoleConnector {
    pub fn new(id: impl Into<String>) -> Arc<Self> {
        let capabilities = ConnectorCapabilities::bidirectional()
            .with_emit_kinds([EventKind::from_static("chat.message")])
            .with_accept_kinds([EventKind::from_static("chat.reply")]);
        Arc::new(Self {
            id: ConnectorId::new(id),
            capabilities,
        })
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
            .subscribe(Filter::by_target(self.id.clone()), SubscribeOptions::default())
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
                        let msg = Envelope::new(
                            self.id.clone(),
                            EventKind::from_static("chat.message"),
                            text,
                        )
                        .with_channel(ChannelId::new(CHANNEL))
                        .with_reply_to(ReplyChannel::new(ChannelId::new(CHANNEL)));
                        ctx.publish(msg).await?;
                    }
                    Ok(None) => {
                        ctx.shutdown.cancel();
                        return Ok(());
                    }
                    Err(e) => {
                        eprintln!("[console] stdin error: {e}");
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
