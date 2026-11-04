//! Synthetic fixtures and bounded probes for cross-crate runtime regressions.
//! This module is excluded from normal/default production builds.
pub mod sessions;
pub use super::messaging::fixture::Fixture as MessagingFixture;
pub use sessions::{message, Fixture as SessionFixture};
use std::{sync::atomic::Ordering, sync::Arc};

use octo_approval::Authority;
use octo_core::{CogitatorContext, Envelope};

use super::{messaging::routes::RuntimeRoutes, AlbertCogitator};
use crate::{
    config::Config,
    messaging::contract::{MessageDraft, TrustedSource},
    sessions::{
        error::SessionResult, presentation::PresentationDetached, transport::HostTransport,
    },
};

pub fn configure(
    host: &mut Arc<AlbertCogitator>,
    transport: Arc<dyn HostTransport>,
    edit: impl FnOnce(&mut Config),
) {
    let host = Arc::get_mut(host).expect("configure synthetic host before use");
    host.transport = transport;
    edit(&mut host.config);
}
pub fn authority(host: &AlbertCogitator) -> Authority {
    host.authority.clone()
}
pub async fn detach(
    host: &AlbertCogitator,
    change: &PresentationDetached,
    context: &CogitatorContext,
) {
    host.detach_presentation(change, context).await;
}
pub async fn reply(
    host: &AlbertCogitator,
    incoming: &Envelope,
    text: String,
    context: &CogitatorContext,
) {
    host.emit_reply(incoming, text, context).await;
}
pub fn feedback(host: &AlbertCogitator, incoming: &Arc<Envelope>, id: &str) -> Arc<Envelope> {
    host.fresh_feedback(incoming, id)
}
pub async fn send(
    host: &AlbertCogitator,
    context: &CogitatorContext,
    source: &TrustedSource,
    draft: &MessageDraft,
    now: i64,
) {
    host.messaging
        .as_ref()
        .expect("synthetic messaging configured")
        .store
        .send(source, draft, &RuntimeRoutes::new(context), now)
        .await
        .unwrap();
}

pub async fn admit(
    host: &AlbertCogitator,
    input: Arc<Envelope>,
    context: &CogitatorContext,
) -> SessionResult<Arc<Envelope>> {
    host.admit_message(input, context).await
}
pub fn attempts(host: &AlbertCogitator) -> u64 {
    host.turn_seq.load(Ordering::Relaxed)
}

pub async fn delivery_pending(host: &AlbertCogitator) -> bool {
    !host
        .messaging
        .as_ref()
        .unwrap()
        .store
        .delivery_page(None, 16)
        .await
        .unwrap()
        .is_empty()
}
