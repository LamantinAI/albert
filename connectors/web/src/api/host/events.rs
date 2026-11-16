use std::{convert::Infallible, pin::Pin};

use futures::Stream;

use crate::api::models::SessionEvent;

pub type EventStream = Pin<Box<dyn Stream<Item = Result<SessionEvent, Infallible>> + Send>>;
pub struct SessionEventRequest {
    pub session_id: String,
    pub after: Option<String>,
    pub runtime_epoch: Option<String>,
}
