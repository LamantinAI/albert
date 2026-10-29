//! The complete capability set for an unprivileged browser conversation.
use std::sync::Arc;

use rig::tool::ToolDyn;

use crate::{
    cogitator::retention::fence_tools,
    scratchpad::{PadScope, ScratchpadStore},
    sessions::SessionContext,
};

pub(super) fn chat_tools(
    store: &Arc<ScratchpadStore>,
    scope: PadScope,
    session: Option<SessionContext>,
) -> Vec<Box<dyn ToolDyn>> {
    let pad = store.scoped_handle(scope);
    let tools: Vec<Box<dyn ToolDyn>> = vec![
        Box::new(pad.goal()),
        Box::new(pad.step()),
        Box::new(pad.mark()),
        Box::new(pad.note()),
        Box::new(pad.clear()),
        Box::new(pad.plan()),
    ];
    fence_tools(tools, session)
}

#[cfg(test)]
mod tests {
    use super::chat_tools;
    use crate::scratchpad::{PadScope, ScratchpadStore};
    use std::sync::Arc;

    #[tokio::test]
    async fn ordinary_chat_has_no_global_tools_and_mutations_stay_in_its_private_scratchpad() {
        let store = Arc::new(ScratchpadStore::new());
        let first = PadScope {
            conversation: "session/first".into(),
            owner: false,
        };
        let second = PadScope {
            conversation: "session/second".into(),
            owner: false,
        };
        let tools = chat_tools(&store, first.clone(), None);
        assert_eq!(tools.len(), 6);
        assert!(tools
            .iter()
            .all(|tool| tool.name().starts_with("scratchpad_")));
        tools
            .iter()
            .find(|tool| tool.name() == "scratchpad_goal")
            .unwrap()
            .call(r#"{"goal":"private finding"}"#.into())
            .await
            .unwrap();
        assert!(store.render_scoped(&first).contains("private finding"));
        assert!(!store.render_scoped(&second).contains("private finding"));
        let privileged = PadScope {
            conversation: first.conversation,
            owner: true,
        };
        assert!(!store.render_scoped(&privileged).contains("private finding"));
    }
}
