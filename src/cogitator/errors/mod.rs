use rig::completion::{CompletionError, PromptError};

use crate::{
    models::{Failure, FailureKind},
    subagents::budget::REPORT_EXHAUSTED,
    transport::{completion_is_transient, is_transient, TIMEOUT},
};

/// A short, polite English message for an LLM failure — never the raw provider payload.
/// On a non-2xx the provider (via rig) hands back the whole response body as the error
/// string; dumping that at the user is a wall of JSON that may echo request detail. The
/// diagnostics log safe classifications instead; the user gets one clean line, with the status
/// code when we can recover it.
pub(super) fn user_facing_llm_error(e: &PromptError) -> String {
    if e.to_string().contains(REPORT_EXHAUSTED) {
        return "Subagent could not produce its final report without tools. Recorded findings remain available for inspection.".into();
    }
    if e.to_string().contains(TIMEOUT) {
        return "Model request timed out.".into();
    }
    if is_transient(&e.to_string()) {
        return "Model connection was interrupted before the response completed.".into();
    }
    if e.to_string().contains("ALBERT_CONTEXT_RESERVE") {
        return "Instructions, tools and response exceed the fixed context reserve. Increase context.reserve_tokens or reduce installed tools/instructions.".into();
    }
    if e.to_string().contains("ALBERT_CONTEXT_WINDOW") {
        return "The dialogue exceeds its context budget. Use /compact or reduce the oversized message/tool result; no additional tool action was started.".into();
    }
    // Our own ceiling, not a provider failure — say so in its own words.
    if let PromptError::MaxTurnsError { .. } = e {
        return "I couldn't finish this within my step budget — try narrowing the request."
            .to_string();
    }
    match provider_status_code(&e.to_string()) {
        Some(code) => format!("LLM provider error: {code}. Please try again in a moment."),
        None => "LLM provider error. Please try again in a moment.".to_string(),
    }
}

pub(super) use crate::transport::provider_status_code;

/// True when the completion failed because the server no longer accepts the access
/// token — an HTTP 401 or an explicit `token_expired` from the provider. Detected
/// from the LIVE response only, never from JWT claims: the server can revoke a
/// token well before its `exp` (which is exactly why [`force_refresh`] exists).
pub(super) fn token_rejected(e: &PromptError) -> bool {
    let PromptError::CompletionError(ce) = e else {
        return false;
    };
    let msg = ce.to_string();
    msg.contains("token_expired") || msg.contains("401 Unauthorized")
}

/// A provider-side failure that clears on its own — overload, a rate limit, a dropped
/// connection. Distinct from a malformed request, which no retry can fix.
pub(super) fn transient(e: &PromptError) -> bool {
    let PromptError::CompletionError(ce) = e else {
        return false;
    };
    completion_is_transient(ce)
}

/// Classify before selecting another model; malformed generic requests and tool
/// errors must not become repeated attempts against every provider.
pub(super) fn model_failure(error: PromptError) -> Failure {
    let raw = error.to_string();
    if raw.contains("ALBERT_CONTEXT_WINDOW") || raw.contains("ALBERT_CONTEXT_RESERVE") {
        return Failure {
            kind: FailureKind::Incompatible,
            message: user_facing_llm_error(&error),
        };
    }
    let lower = raw.to_ascii_lowercase();
    let completion = matches!(error, PromptError::CompletionError(_));
    let capability = [
        "does not support",
        "not supported",
        "unsupported",
        "no endpoints found",
        "context_length_exceeded",
        "maximum context length",
    ]
    .iter()
    .any(|hint| lower.contains(hint));
    let kind = if completion
        && (token_rejected(&error) || matches!(provider_status_code(&raw), Some(401 | 403)))
    {
        FailureKind::Authentication
    } else if transient(&error) {
        FailureKind::Transient
    } else if (completion && capability)
        || matches!(
            &error,
            PromptError::CompletionError(
                CompletionError::ResponseError(_) | CompletionError::JsonError(_)
            )
        )
    {
        FailureKind::Incompatible
    } else if completion && matches!(provider_status_code(&raw), Some(404)) {
        FailureKind::Unavailable
    } else {
        FailureKind::Fatal
    };
    Failure {
        kind,
        message: format!("{kind:?}: {}", user_facing_llm_error(&error)),
    }
}

#[cfg(test)]
mod fixture;
#[cfg(test)]
mod tests {
    use std::{future::ready, time::Duration};

    use rig::completion::{CompletionModel, Message, PromptError};
    use tokio::time::timeout;

    use super::{fixture::Server, model_failure};
    use crate::{
        config::AuthMode,
        models::{Failure, FailureKind, ModelSpec, PoolConfig, Snapshot},
    };

    fn pool(first: &str) -> Snapshot {
        Snapshot {
            selected: first.into(),
            config: PoolConfig {
                default: first.into(),
                max_attempts: 2,
                retries_per_model: 0,
                retry_delay_ms: 0,
                models: [first, "healthy"]
                    .into_iter()
                    .map(|id| ModelSpec {
                        id: id.into(),
                        model: id.into(),
                        provider: AuthMode::Subscription,
                        context_window: None,
                        base_url: None,
                        api_key_env: None,
                        vision: false,
                        tools: false,
                        request_timeout_ms: 100,
                    })
                    .collect(),
            },
        }
    }
    async fn attempt(server: &Server, id: &str) -> Result<(), Failure> {
        let model = server.model(id);
        model
            .completion(model.completion_request(Message::user("compact")).build())
            .await
            .map(|_| ())
            .map_err(|e| model_failure(PromptError::CompletionError(e)))
    }
    #[tokio::test]
    async fn stalled_sse_is_transient_and_falls_back_without_losing_cause() {
        let server = Server::new().await;
        let error = timeout(Duration::from_secs(2), attempt(&server, "stall"))
            .await
            .unwrap()
            .unwrap_err();
        assert_eq!(error.kind, FailureKind::Transient);
        assert!(error.message.contains("timed out"));
        assert!(!error.message.contains("127.0.0.1"));
        server.requests.lock().unwrap().clear();
        timeout(
            Duration::from_secs(2),
            pool("stall").run_with_tools(
                false,
                false,
                |m, _| {
                    let server = &server;
                    async move { attempt(server, &m.id).await }
                },
                || false,
                |_| ready(()),
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(*server.requests.lock().unwrap(), ["stall", "healthy"]);
    }
    #[tokio::test]
    async fn incomplete_stream_is_not_success_and_completed_stream_need_not_close() {
        let server = Server::new().await;
        let error = attempt(&server, "truncated").await.unwrap_err();
        assert_eq!(error.kind, FailureKind::Transient);
        assert!(error.message.contains("interrupted"));
        timeout(Duration::from_secs(2), attempt(&server, "keep-open"))
            .await
            .unwrap()
            .unwrap();
    }
    #[tokio::test]
    async fn transport_failure_after_tools_never_replays_the_attempt() {
        let server = Server::new().await;
        let error = pool("stall")
            .run_with_tools(
                false,
                false,
                |m, _| {
                    let server = &server;
                    async move { attempt(server, &m.id).await }
                },
                || true,
                |_| ready(()),
            )
            .await
            .unwrap_err();
        assert!(error.contains("tools may already have run"));
        assert_eq!(*server.requests.lock().unwrap(), ["stall"]);
    }
    #[tokio::test]
    async fn http_auth_and_bad_requests_are_not_misclassified_as_network_failures() {
        let server = Server::new().await;
        assert_eq!(
            attempt(&server, "unauthorized").await.unwrap_err().kind,
            FailureKind::Authentication
        );
        assert_eq!(
            attempt(&server, "bad-request").await.unwrap_err().kind,
            FailureKind::Fatal
        );
    }

    #[tokio::test]
    async fn compact_total_deadline_still_bounds_retries_and_preserves_sql_context() {
        use crate::{
            cogitator::provider_fixture::setup,
            history::{HistoryStore, SqliteHistory, Turn},
            models::ModelPool,
            status::StatusFeed,
        };
        use std::sync::Arc;
        use tempfile::tempdir;
        let (mut host, _, server) = setup("never").await;
        let dir = tempdir().unwrap();
        let db = Arc::new(
            SqliteHistory::open_retained(dir.path().join("history.db"))
                .await
                .unwrap(),
        );
        let me = Arc::get_mut(&mut host).unwrap();
        me.history = db.clone();
        me.config.context.timeout_secs = 1;
        me.config.context.request_timeout_ms = 600;
        me.config.models.as_mut().unwrap().models[1].model = "never-secondary".into();
        me.models = ModelPool::new(&me.config);
        db.append("room", &[Turn::user("keep these facts ".repeat(100))])
            .await
            .unwrap();
        let error = timeout(
            Duration::from_secs(3),
            host.compact_history("room", true, false, &StatusFeed::silent()),
        )
        .await
        .unwrap()
        .unwrap_err();
        assert!(error.contains("total time budget"));
        assert!(db.context("room").await.unwrap().compact.is_none());
        assert_eq!(db.load("room").await.len(), 1);
        assert_eq!(server.requests.lock().unwrap().len(), 2);
    }
    #[tokio::test]
    async fn sse_continuation_timeout_recovers_on_the_same_model() {
        use crate::{context::BudgetedModel, status::StatusFeed};
        use rig::{
            message::{AssistantContent, ToolCall, ToolFunction, ToolResultContent, UserContent},
            OneOrMany,
        };
        use serde_json::json;
        let server = Server::new().await;
        let model = BudgetedModel::new(server.model("stall-once"), None, false).with_recovery(
            1,
            0,
            StatusFeed::silent(),
        );
        let history = [
            Message::Assistant {
                id: None,
                content: OneOrMany::one(AssistantContent::ToolCall(
                    ToolCall::new(
                        "fc_1".into(),
                        ToolFunction {
                            name: "already_finished".into(),
                            arguments: json!({}),
                        },
                    )
                    .with_call_id("call_1".into()),
                )),
            },
            Message::User {
                content: OneOrMany::one(UserContent::tool_result_with_call_id(
                    "fc_1",
                    "call_1".into(),
                    OneOrMany::one(ToolResultContent::text("saved result")),
                )),
            },
        ];
        let request = model
            .completion_request(Message::user("continue from the saved result"))
            .messages(history)
            .build();
        timeout(Duration::from_secs(2), model.completion(request))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            *server.requests.lock().unwrap(),
            ["stall-once", "stall-once"]
        );
    }
}
