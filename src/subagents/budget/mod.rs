//! Per-worker model/tool rounds, independent of rig's depth counter.
use std::sync::{
    atomic::{AtomicBool, AtomicUsize, Ordering},
    Arc,
};

use rig::{
    completion::{CompletionError, CompletionRequest, ToolDefinition},
    message::AssistantContent,
    tool::{ToolDyn, ToolError},
    wasm_compat::WasmBoxedFuture,
    OneOrMany,
};
use serde_json::json;

pub const REPORT_EXHAUSTED: &str = "ALBERT_CHILD_REPORT_EXHAUSTED";

pub struct Budget {
    limit: usize,
    used: AtomicUsize,
    finalizing: AtomicBool,
    reports: AtomicUsize,
}
impl Budget {
    pub fn new(limit: usize) -> Arc<Self> {
        Arc::new(Self {
            limit,
            used: AtomicUsize::new(0),
            finalizing: AtomicBool::new(false),
            reports: AtomicUsize::new(0),
        })
    }
    pub fn used(&self) -> usize {
        self.used.load(Ordering::Relaxed)
    }
    pub fn exhausted(&self) -> bool {
        self.finalizing.load(Ordering::Relaxed)
    }
    pub fn prepare(&self, request: &mut CompletionRequest) -> Result<(), CompletionError> {
        let remaining = self.limit.saturating_sub(self.used());
        let note = if remaining == 0 {
            self.finalizing.store(true, Ordering::Relaxed);
            if self.reports.fetch_add(1, Ordering::Relaxed) > 0 {
                return Err(CompletionError::ProviderError(format!(
                    "{REPORT_EXHAUSTED}: the final report must not request tools"
                )));
            }
            request.tools.clear();
            request.tool_choice = None;
            "Your tool-round budget is exhausted. This is your single final reporting response and NO tools can execute. Return the useful findings you already obtained, with source links, blockers and missing items. Do not invent evidence or claim blocked pages were read. If nothing was verified, say so. Do not print tool-call markup.".to_owned()
        } else {
            format!("Tool-enabled rounds remaining: {remaining}/{}. Parallel calls in one response share one round. Finish early once you have enough evidence. An HTTP success status or a page containing only navigation is not evidence that an article was read. If sources are blocked or empty, do not spend the budget repeating near-identical attempts; return what is verified and explain gaps. After the last tool round you receive one tool-free reporting response. Only tools actually listed for this run exist; never invent unavailable tools or print tool-call markup.", self.limit)
        };
        request
            .preamble
            .get_or_insert_with(String::new)
            .push_str(&format!("\n\n[Host execution budget]\n{note}"));
        Ok(())
    }
    pub fn record(&self, response: &OneOrMany<AssistantContent>) {
        if !self.exhausted()
            && response
                .iter()
                .any(|p| matches!(p, AssistantContent::ToolCall(_)))
        {
            self.used.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Removing a schema from the final request is not enforcement. A model that
/// nevertheless asks for an old tool must be stopped before dispatch/execution.
pub struct LimitedTool {
    pub inner: Box<dyn ToolDyn>,
    pub budget: Arc<Budget>,
}
impl ToolDyn for LimitedTool {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn definition(&self, prompt: String) -> WasmBoxedFuture<'_, ToolDefinition> {
        self.inner.definition(prompt)
    }
    fn call(&self, args: String) -> WasmBoxedFuture<'_, Result<String, ToolError>> {
        Box::pin(async move {
            if self.budget.exhausted() {
                return Ok(json!({"error":"Tool budget exhausted; this is the final reporting response.","executed":false,"instruction":"Return available findings and limitations without calling tools."}).to_string());
            }
            self.inner.call(args).await
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::{
        completion::Message,
        message::{ToolCall, ToolFunction},
        tool::Tool,
    };
    use std::{convert::Infallible, sync::atomic::AtomicUsize};

    fn request() -> CompletionRequest {
        CompletionRequest {
            model: None,
            preamble: None,
            chat_history: OneOrMany::one(Message::user("task")),
            documents: vec![],
            tools: vec![ToolDefinition {
                name: "counter".into(),
                description: "test".into(),
                parameters: json!({}),
            }],
            temperature: None,
            max_tokens: None,
            tool_choice: None,
            additional_params: None,
            output_schema: None,
        }
    }
    fn calls(count: usize) -> OneOrMany<AssistantContent> {
        OneOrMany::many((0..count).map(|i| {
            AssistantContent::ToolCall(ToolCall::new(
                i.to_string(),
                ToolFunction {
                    name: "counter".into(),
                    arguments: json!({}),
                },
            ))
        }))
        .unwrap()
    }
    #[test]
    fn rounds_are_not_individual_tools_and_failed_requests_do_not_spend_them() {
        let budget = Budget::new(2);
        budget.prepare(&mut request()).unwrap();
        budget.prepare(&mut request()).unwrap();
        assert_eq!(budget.used(), 0);
        budget.record(&calls(3));
        assert_eq!(budget.used(), 1);
        let mut next = request();
        budget.prepare(&mut next).unwrap();
        assert!(next.preamble.unwrap().contains("remaining: 1/2"));
        budget.record(&calls(1));
        let mut final_request = request();
        budget.prepare(&mut final_request).unwrap();
        assert!(final_request.tools.is_empty());
        assert!(budget.exhausted());
        assert!(budget
            .prepare(&mut request())
            .unwrap_err()
            .to_string()
            .contains(REPORT_EXHAUSTED));
    }
    struct Counter(Arc<AtomicUsize>);
    impl Tool for Counter {
        const NAME: &'static str = "counter";
        type Error = Infallible;
        type Args = serde_json::Value;
        type Output = bool;
        async fn definition(&self, _: String) -> ToolDefinition {
            request().tools.remove(0)
        }
        async fn call(&self, _: Self::Args) -> Result<bool, Infallible> {
            self.0.fetch_add(1, Ordering::Relaxed);
            Ok(true)
        }
    }
    #[tokio::test]
    async fn final_report_cannot_execute_a_tool_even_when_model_guesses_its_name() {
        let budget = Budget::new(1);
        let count = Arc::new(AtomicUsize::new(0));
        let tool = LimitedTool {
            inner: Box::new(Counter(count.clone())),
            budget: budget.clone(),
        };
        tool.call("{}".into()).await.unwrap();
        assert_eq!(count.load(Ordering::Relaxed), 1);
        budget.record(&calls(1));
        budget.prepare(&mut request()).unwrap();
        let denied = tool.call("{}".into()).await.unwrap();
        assert!(denied.contains("\"executed\":false"));
        assert_eq!(count.load(Ordering::Relaxed), 1);
    }
}
