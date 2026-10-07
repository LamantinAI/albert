use std::{future::Future, time::Duration};

use tokio::time::sleep;

use super::{ModelSpec, Snapshot};
use crate::config::AuthMode;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FailureKind {
    Transient,
    Authentication,
    Incompatible,
    Unavailable,
    Fatal,
}

#[derive(Debug)]
pub struct Failure {
    pub kind: FailureKind,
    /// Safe, short summary; never raw provider bodies or credentials.
    pub message: String,
}

impl Snapshot {
    /// Selection and retries are frozen for this turn. No automatic attempt may
    /// replay a tool round produced by an earlier attempt.
    pub async fn run<T, F, Fut, G, N, Notify>(
        &self,
        vision: bool,
        attempt: F,
        touched_tools: G,
        notify: N,
    ) -> Result<T, String>
    where
        F: FnMut(ModelSpec, bool) -> Fut,
        Fut: Future<Output = Result<T, Failure>>,
        G: Fn() -> bool,
        N: FnMut(String) -> Notify,
        Notify: Future<Output = ()>,
    {
        self.run_with_tools(vision, true, attempt, touched_tools, notify)
            .await
    }

    pub async fn run_with_tools<T, F, Fut, G, N, Notify>(
        &self,
        vision: bool,
        tools: bool,
        mut attempt: F,
        touched_tools: G,
        mut notify: N,
    ) -> Result<T, String>
    where
        F: FnMut(ModelSpec, bool) -> Fut,
        Fut: Future<Output = Result<T, Failure>>,
        G: Fn() -> bool,
        N: FnMut(String) -> Notify,
        Notify: Future<Output = ()>,
    {
        let mut attempts = 0;
        let mut refreshed = false;
        let mut failures = Vec::new();
        for model in self.ordered() {
            if (tools && !model.tools) || (vision && !model.vision) {
                let why = format!(
                    "{}: incompatible (requires tools{}).",
                    model.id,
                    if vision { " and vision" } else { "" }
                );
                notify(why.clone()).await;
                failures.push(why);
                continue;
            }
            let mut retries = 0;
            let mut force_refresh = false;
            while attempts < self.config.max_attempts {
                attempts += 1;
                notify(format!(
                    "Model: {} ({}) · attempt {}/{}",
                    model.id, model.model, attempts, self.config.max_attempts
                ))
                .await;
                match attempt(model.clone(), force_refresh).await {
                    Ok(value) => return Ok(value),
                    Err(error) => {
                        let why = format!("{}: {}", model.id, error.message);
                        notify(why.clone()).await;
                        failures.push(why);
                        if touched_tools() {
                            return Err(format!("{} Automatic fallback stopped because tools may already have run; their recorded results are preserved. Continue explicitly after checking the outcome.", failures.last().unwrap()));
                        }
                        if error.kind == FailureKind::Fatal {
                            return Err(error.message);
                        }
                        force_refresh = false;
                        if error.kind == FailureKind::Authentication
                            && model.provider == AuthMode::Subscription
                            && !refreshed
                        {
                            refreshed = true;
                            force_refresh = true;
                            continue;
                        }
                        if error.kind == FailureKind::Transient
                            && retries < self.config.retries_per_model
                            && attempts < self.config.max_attempts
                        {
                            retries += 1;
                            sleep(Duration::from_millis(self.config.retry_delay_ms)).await;
                            continue;
                        }
                        break;
                    }
                }
            }
            if attempts >= self.config.max_attempts {
                break;
            }
        }
        Err(format!(
            "No eligible model succeeded ({attempts}/{} attempts). {}",
            self.config.max_attempts,
            failures.join(" ")
        ))
    }
}
