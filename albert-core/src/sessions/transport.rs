//! Host-only transport ports. Values here carry trusted ingress and presentation
//! intent; connector wire objects never enter session policy or cognition.
use std::{sync::atomic::AtomicBool, sync::atomic::Ordering, sync::Arc};

use octo_core::{ConnectorId, Envelope};
use octo_history::RequestAlias;
use serde_json::Value;

use crate::{db::entity::ExcursionKey, db::entity::NativePosition};

#[derive(Clone)]
pub enum NativeIdentityError {
    Unsupported,
    Denied,
}

#[derive(Clone)]
pub struct NativeNavigation {
    pub identity: Result<ExcursionKey, NativeIdentityError>,
    pub position: NativePosition,
    pub message_id: i64,
    pub alias: RequestAlias,
}

#[derive(Clone)]
pub struct NativeConstituent {
    pub connector: ConnectorId,
    pub channel: String,
    pub alias: RequestAlias,
    pub source: Value,
}
#[derive(Clone)]
pub struct NativeIngress {
    pub constituents: Vec<NativeConstituent>,
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct OutputRetention {
    pub key: String,
    pub epoch: String,
    pub ephemeral: bool,
    pub cleanup_scope: Option<String>,
}
#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct OutputScope {
    pub scope: OutputRetention,
    pub attempt_id: Option<String>,
}

/// Implemented by a presentation adapter so closure and queued output share one
/// lifetime. Attaching the native token is an outward operation owned by it.
pub trait FeedbackToken: Send + Sync {
    fn id(&self) -> &str;
    fn close(&self);
    fn is_open(&self) -> bool;
    fn attach(&self, output: Envelope) -> Envelope;
    fn fresh(&self, id: &str) -> TurnFeedback;
}
#[derive(Clone)]
pub struct TurnFeedback(Arc<dyn FeedbackToken>);
impl TurnFeedback {
    pub fn from_token(token: impl FeedbackToken + 'static) -> Self {
        Self(Arc::new(token))
    }
    pub fn new(id: impl Into<String>) -> Self {
        Self::from_token(LocalFeedback {
            id: id.into(),
            open: AtomicBool::new(true),
        })
    }
    pub fn id(&self) -> &str {
        self.0.id()
    }
    pub fn close(&self) {
        self.0.close();
    }
    pub fn is_open(&self) -> bool {
        self.0.is_open()
    }
    pub fn fresh(&self, id: &str) -> Self {
        self.0.fresh(id)
    }
    pub fn output(&self, output: Envelope) -> Envelope {
        self.0.attach(output).with_context(self.clone())
    }
}
struct LocalFeedback {
    id: String,
    open: AtomicBool,
}
impl FeedbackToken for LocalFeedback {
    fn id(&self) -> &str {
        &self.id
    }
    fn close(&self) {
        self.open.store(false, Ordering::Release);
    }
    fn is_open(&self) -> bool {
        self.open.load(Ordering::Acquire)
    }
    fn attach(&self, output: Envelope) -> Envelope {
        output
    }
    fn fresh(&self, id: &str) -> TurnFeedback {
        TurnFeedback::new(id)
    }
}

pub const TURN_END: &str = "chat.turn.end";

pub trait HostTransport: Send + Sync {
    fn ingress(&self, envelope: Arc<Envelope>) -> Arc<Envelope>;
    fn feedback(&self, id: &str) -> TurnFeedback;
}
pub struct LocalTransport;
impl HostTransport for LocalTransport {
    fn ingress(&self, envelope: Arc<Envelope>) -> Arc<Envelope> {
        envelope
    }
    fn feedback(&self, id: &str) -> TurnFeedback {
        TurnFeedback::new(id)
    }
}
