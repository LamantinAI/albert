use super::{clip, display_args, summarize_result, StatusFeed};
use rig::{
    agent::{HookAction, PromptHook, ToolCallHookAction},
    completion::{CompletionModel, CompletionResponse, Message},
    message::{AssistantContent, ReasoningContent},
};

impl<M: CompletionModel> PromptHook<M> for StatusFeed {
    /// Before each tool runs: show which one, with a clipped arg preview.
    fn on_tool_call(
        &self,
        tool_name: &str,
        tool_call_id: Option<String>,
        internal_call_id: &str,
        args: &str,
    ) -> impl std::future::Future<Output = ToolCallHookAction> + Send {
        let mut trace = self.trace.lock().unwrap();
        if self.is_live() {
            trace.start(tool_name, tool_call_id.as_deref(), internal_call_id, args);
        }
        drop(trace);
        let feed = self.clone();
        let line = format!(
            "{tool_name} {}",
            clip(&display_args(tool_name, args).replace('\n', " "), 160)
        );
        async move {
            if !feed.is_live() {
                return ToolCallHookAction::terminate("Session ended.");
            }
            feed.emit(line).await;
            ToolCallHookAction::cont()
        }
    }

    /// After each tool returns: record a compact, durable line of what was done —
    /// name + (kept) arguments + a hard-compressed result. Args are cheap and are the
    /// "what did I do"; results are the expensive half, so they collapse to ok/err.
    fn on_tool_result(
        &self,
        tool_name: &str,
        _tool_call_id: Option<String>,
        internal_call_id: &str,
        args: &str,
        result: &str,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let mut trace = self.trace.lock().unwrap();
        let recorded = self.is_live() && trace.result(internal_call_id, result);
        drop(trace);
        if recorded {
            if let Some(observer) = &self.result_observer {
                observer(tool_name, args, result);
            }
        }
        if let Ok(mut v) = self.actions.lock() {
            if self.is_live() {
                v.push(format!(
                    "{tool_name} {} -> {}",
                    clip(&display_args(tool_name, args).replace('\n', " "), 120),
                    summarize_result(result),
                ));
            }
        }
        let live = self.is_live();
        async move {
            if live {
                HookAction::cont()
            } else {
                HookAction::terminate("Session ended.")
            }
        }
    }

    /// After each model round: surface any reasoning the provider returned
    /// (e.g. Codex reasoning summaries) as the agent's "thoughts".
    fn on_completion_response(
        &self,
        _prompt: &Message,
        response: &CompletionResponse<M::Response>,
    ) -> impl std::future::Future<Output = HookAction> + Send {
        let feed = self.clone();
        let mut trace = self.trace.lock().unwrap();
        if self.is_live() {
            trace.response(response.message_id.clone(), response.choice.clone());
        }
        drop(trace);
        let thoughts: Vec<String> = response
            .choice
            .iter()
            .filter_map(|c| match c {
                AssistantContent::Reasoning(r) => {
                    let text = r
                        .content
                        .iter()
                        .filter_map(|rc| match rc {
                            ReasoningContent::Text { text, .. } => Some(text.as_str()),
                            ReasoningContent::Summary(text) => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    (!text.trim().is_empty()).then(|| format!("Thought: {}", clip(&text, 280)))
                }
                _ => None,
            })
            .collect();
        async move {
            if !feed.is_live() {
                return HookAction::terminate("Session ended.");
            }
            for line in thoughts {
                feed.emit(line).await;
            }
            HookAction::cont()
        }
    }
}
