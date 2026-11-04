//! Albert authenticated web control plane.
pub mod api;
mod service;
pub mod settings;
mod web;
pub use service::auth::{AuthService, AuthSettings};
pub use web::{configured_owner, HostedPanel, Panel};
