//! Retry a failed inference continuation, never replay the tool loop that led to it.
use std::{
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use rig::{
    completion::{
        CompletionError, CompletionModel, CompletionRequest, CompletionResponse, Message,
    },
    message::UserContent,
};
use tokio::time::sleep;
use tracing::warn;

use crate::{
    status::StatusFeed,
    transport::{completion_is_transient, TIMEOUT},
};

#[derive(Clone)]
pub(super) struct Recovery {
    remaining: Arc<AtomicUsize>,
    delay: Duration,
    feed: StatusFeed,
}
impl Recovery {
    pub fn new(retries: usize, delay_ms: u64, feed: StatusFeed) -> Self {
        Self {
            remaining: Arc::new(AtomicUsize::new(retries)),
            delay: Duration::from_millis(delay_ms),
            feed,
        }
    }
    pub async fn complete<M: CompletionModel>(
        &self,
        model: &M,
        request: CompletionRequest,
    ) -> Result<CompletionResponse<M::Response>, CompletionError> {
        // Initial-request failures retain normal pool fallback. Only the current
        // request *after* tool results is resumed, on the exact same model/client.
        let continuation = request.chat_history.iter().any(|message| match message {
            Message::User { content } => content
                .iter()
                .any(|c| matches!(c, UserContent::ToolResult(_))),
            _ => false,
        });
        if !continuation {
            return model.completion(request).await;
        }
        loop {
            if self.remaining.load(Ordering::Relaxed) == 0 {
                return model.completion(request).await;
            }
            match model.completion(request.clone()).await {
                Ok(response) => return Ok(response),
                Err(error) => {
                    if !continuation
                        || !completion_is_transient(&error)
                        || self
                            .remaining
                            .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |n| {
                                n.checked_sub(1)
                            })
                            .is_err()
                    {
                        return Err(error);
                    }
                    let timeout = error.to_string().contains(TIMEOUT);
                    warn!(
                        timeout,
                        retries_left = self.remaining.load(Ordering::Relaxed),
                        "retrying current model continuation; completed tool results retained"
                    );
                    self.feed.progress(if timeout {
                        "Model request timed out. Retrying the current response with completed tool results; previous actions will not be replayed."
                    } else {
                        "Model request failed. Retrying the current response with completed tool results; previous actions will not be replayed."
                    }).await;
                    // Both the delay and request remain inside the parent future,
                    // so a new user message or cancellation interrupts recovery.
                    sleep(self.delay).await;
                }
            }
        }
    }
}
