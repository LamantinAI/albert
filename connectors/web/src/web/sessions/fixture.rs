//! Session boundary tests share the real browser account fixture.
use super::SessionHost;
use crate::api::principal::Principal;
use std::sync::Arc;
use tempfile::TempDir;
pub(super) async fn fixture() -> (TempDir, Arc<SessionHost>, Principal) {
    let (dir, runtime, auth, caller) = crate::web::testing::fixture().await;
    (dir, SessionHost::new(runtime, auth), caller)
}
