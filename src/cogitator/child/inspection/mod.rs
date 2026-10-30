use serde_json::Value;

use crate::{
    cogitator::{retention::temporary, AlbertCogitator},
    subagents::{
        inspection::{inspect_run, IndexRequest},
        SubagentTool,
    },
};

impl AlbertCogitator {
    pub(super) async fn inspect_child(
        &self,
        tool: &SubagentTool,
        id: &str,
        offset: usize,
        limit: Option<usize>,
    ) -> Result<Value, String> {
        let run = self.children.get(id, &tool.conversation, tool.owner)?;
        let messages = self.inspection_messages(&run).await?;
        let root = self.config.code_workspace.join("tool-results");
        inspect_run(
            &messages,
            (!temporary(run.session.as_ref())).then_some(root.as_path()),
            &self.config.subagents.inspection,
            IndexRequest {
                run_id: &run.id,
                run: run.inspection_summary(),
                offset,
                limit,
                inline_bytes: self.config.context.artifacts.threshold_bytes,
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use serde_json::{from_str, json, to_string, Value};
    use tempfile::tempdir;

    use crate::{
        artifacts::Artifacts, cogitator::provider_fixture::setup, subagents::SubagentTool,
    };
    #[tokio::test]
    async fn parent_receives_inline_index_and_direct_failed_child_read_ref() {
        let (mut host, ctx, server) = setup("healthy").await;
        let directory = tempdir().unwrap();
        let instance = Arc::get_mut(&mut host).unwrap();
        instance.config.code_workspace = directory.path().into();
        instance.config.context.artifacts.threshold_bytes = 1024;
        let tool = SubagentTool::new(
            Arc::downgrade(&host),
            ctx,
            ("telegram".into(), "room".into()),
            "parent-attempt".into(),
            true,
        );
        let (run, _, _) = host
            .children
            .reserve_scoped(
                &host.config.subagents,
                "parent-attempt".into(),
                tool.conversation.clone(),
                true,
                &host.id,
                None,
            )
            .unwrap();
        for index in 0..8 {
            let id = format!("call-{index}");
            run.feed.start_external(
                &id,
                "dispatch_to_connector",
                json!({"target":"browser","query":"long ".repeat(1000)}),
            );
            run.feed.finish_external(&id,&json!({"kind":"browser.fetch.result","result":{"text":format!("entry-{index} {}","verified evidence ".repeat(5000)),"html":"<p>large</p>".repeat(5000)}}).to_string());
        }
        run.complete(json!({"outcome":{"status":"failed","error":"Model returned an empty answer","tool_rounds":10},"journal_saved":false}));
        let before = to_string(&run.feed.snapshot()).unwrap();
        let artifacts = Artifacts::new(
            directory.path().into(),
            &tool.conversation,
            true,
            None,
            host.config.context.artifacts.clone(),
        );
        let tools = artifacts.wrap(vec![Box::new(tool.clone())]);
        let raw = tools[0]
            .call(json!({"action":"inspect","run_id":run.id,"offset":0,"limit":3}).to_string())
            .await
            .unwrap();
        assert!(raw.len() <= 1024);
        let index: Value = from_str(&raw).unwrap();
        assert!(index.get("offloaded").is_none(), "{index}");
        assert_eq!(index["run"]["status"], "failed");
        assert_eq!(index["journal"]["effective_limit"], 3);
        assert_eq!(index["journal"]["returned"], 1);
        assert_eq!(index["journal"]["next_offset"], 1);
        let mut selected = index["journal"]["entries"][0]["read_ref"].clone();
        selected["limit"] = 7.into();
        let read: Value = from_str(&tools[0].call(selected.to_string()).await.unwrap()).unwrap();
        assert_eq!(read["content"], "entry-0");
        assert_eq!(before, to_string(&run.feed.snapshot()).unwrap());
        assert!(server.requests.lock().unwrap().is_empty());
        let mut foreign = tool.clone();
        foreign.conversation = ("telegram".into(), "another".into());
        assert!(host
            .inspect_child(&foreign, &run.id, 0, Some(1))
            .await
            .is_err());
    }
}
