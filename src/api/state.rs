use std::sync::Arc;

use super::host::Host;
use crate::service::auth::AuthService;

#[derive(Clone)]
pub struct ApiState {
    pub auth: Arc<AuthService>,
    pub host: Arc<dyn Host>,
}
impl ApiState {
    pub fn new(auth: Arc<AuthService>, host: Arc<dyn Host>) -> Self {
        Self { auth, host }
    }
}
