//! Bounded model-facing tool results; payload files are scoped to a conversation.
mod store;
mod tool;

use std::{path::PathBuf, sync::Arc};

use rig::{
    completion::ToolDefinition,
    tool::{ToolDyn, ToolError},
    wasm_compat::WasmBoxedFuture,
};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use tracing::warn;

pub use tool::ArtifactTool;

#[derive(Clone, Debug, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Settings {
    pub enabled: bool,
    pub threshold_bytes: usize,
    pub preview_chars: usize,
    pub read_chars: usize,
    pub max_file_bytes: usize,
    pub max_scope_bytes: usize,
    pub retention_secs: u64,
}
impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled: true,
            threshold_bytes: 16384,
            preview_chars: 2048,
            read_chars: 8192,
            max_file_bytes: 64 * 1024 * 1024,
            max_scope_bytes: 512 * 1024 * 1024,
            retention_secs: 7 * 86400,
        }
    }
}
impl Settings {
    pub fn validate(&self) -> Result<(), String> {
        if self.threshold_bytes < 1024
            || self.preview_chars == 0
            || self.preview_chars > 4096
            || self.read_chars == 0
            || self.read_chars > 32768
            || self.max_file_bytes < self.threshold_bytes
            || self.max_scope_bytes < self.max_file_bytes
            || self.retention_secs == 0
        {
            return Err("context.artifacts: require threshold >= 1024, preview 1..4096, read 1..32768, max_scope >= max_file >= threshold, and positive retention".into());
        }
        Ok(())
    }
}
#[derive(Clone)]
pub struct Artifacts {
    root: PathBuf,
    prefix: String,
    owner: bool,
    settings: Settings,
}
impl Artifacts {
    pub fn new(
        workspace: PathBuf,
        conversation: &(String, String),
        owner: bool,
        child: Option<&str>,
        settings: Settings,
    ) -> Self {
        let key = serde_json::to_vec(conversation).expect("string tuple serializes");
        let root = workspace
            .join("tool-results")
            .join(format!("{:x}", Sha256::digest(key)));
        let prefix = child
            .map(|s| format!("{:x}/", Sha256::digest(s.as_bytes())))
            .unwrap_or_default();
        Self {
            root,
            prefix,
            owner,
            settings,
        }
    }
    pub fn wrap(&self, mut tools: Vec<Box<dyn ToolDyn>>) -> Vec<Box<dyn ToolDyn>> {
        if !self.settings.enabled || tools.is_empty() {
            return tools;
        }
        tools = tools
            .into_iter()
            .map(|inner| {
                Box::new(BoundedTool {
                    inner: Arc::from(inner),
                    artifacts: self.clone(),
                }) as Box<dyn ToolDyn>
            })
            .collect();
        tools.push(Box::new(ArtifactTool(self.clone())));
        tools
    }
    pub async fn bound(&self, name: String, args: String, output: String) -> String {
        if output.len() <= self.settings.threshold_bytes {
            return output;
        }
        let store = self.clone();
        match tokio::task::spawn_blocking(move || store.save(&name, &args, &output)).await {
            Ok(value) => value.to_string(),
            Err(error) => {
                warn!(%error, "artifact writer task failed");
                serde_json::json!({"offloaded":false,"truncated":true,"artifact_error":"Payload storage failed after the tool returned. Its effects are NOT undone; do not repeat the action blindly."}).to_string()
            }
        }
    }
}
struct BoundedTool {
    inner: Arc<dyn ToolDyn>,
    artifacts: Artifacts,
}
impl ToolDyn for BoundedTool {
    fn name(&self) -> String {
        self.inner.name()
    }
    fn definition(&self, prompt: String) -> WasmBoxedFuture<'_, ToolDefinition> {
        self.inner.definition(prompt)
    }
    fn call(&self, args: String) -> WasmBoxedFuture<'_, Result<String, ToolError>> {
        Box::pin(async move {
            let output = match self.inner.call(args.clone()).await {
                Ok(value) => value,
                Err(error) => {
                    if error.to_string().len() <= self.artifacts.settings.threshold_bytes {
                        return Err(error);
                    }
                    serde_json::json!({"tool_error":error.to_string()}).to_string()
                }
            };
            Ok(self.artifacts.bound(self.name(), args, output).await)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rig::{
        completion::ToolDefinition,
        tool::{Tool, ToolDyn},
        wasm_compat::WasmBoxedFuture,
    };
    use serde_json::{json, Value};
    use std::{
        fs,
        sync::atomic::{AtomicUsize, Ordering},
        time::{Duration, SystemTime},
    };
    use tempfile::tempdir;

    fn store(path: PathBuf, child: Option<&str>) -> Artifacts {
        Artifacts::new(
            path,
            &("telegram".into(), "room".into()),
            false,
            child,
            Settings::default(),
        )
    }
    async fn read(store: &Artifacts, id: &str, field: Value) -> Value {
        Tool::call(
            &ArtifactTool(store.clone()),
            serde_json::from_value(
                json!({"action":"read","id":id,"field":field,"offset":0,"limit":120}),
            )
            .unwrap(),
        )
        .await
        .unwrap()
    }
    #[tokio::test]
    async fn huge_result_becomes_bounded_reference_and_child_scope_is_enforced() {
        let dir = tempdir().unwrap();
        let child = store(dir.path().into(), Some("child-a"));
        let original =
            json!({"result":{"url":"https://example.test/source","text":"Статья ".repeat(100000)}})
                .to_string();
        let bounded = child
            .bound(
                "dispatch_to_connector".into(),
                "{}".into(),
                original.clone(),
            )
            .await;
        assert!(bounded.len() < 14000);
        let value: Value = serde_json::from_str(&bounded).unwrap();
        assert_eq!(value["offloaded"], true);
        assert_eq!(value["metadata"]["url"], "https://example.test/source");
        let id = value["artifact_id"].as_str().unwrap();
        let part = read(&child, id, json!(["result", "text"])).await;
        assert_eq!(part["content"].as_str().unwrap().chars().count(), 120);
        assert_eq!(part["next_offset"], 120);
        assert!(
            read(&store(dir.path().into(), Some("child-b")), id, json!([]))
                .await
                .get("error")
                .is_some()
        );
        assert!(read(&store(dir.path().into(), None), id, json!([]))
            .await
            .get("error")
            .is_none());
        let other = Artifacts::new(
            dir.path().into(),
            &("telegram".into(), "other-room".into()),
            false,
            None,
            Settings::default(),
        );
        assert!(read(&other, id, json!([])).await.get("error").is_some());
        let bytes = fs::read(value["path"].as_str().unwrap()).unwrap();
        assert_eq!(
            serde_json::from_slice::<Value>(&bytes).unwrap()["data"],
            original
        );
        let found=Tool::call(&ArtifactTool(child),serde_json::from_value(json!({"action":"search","id":id,"field":["result","text"],"query":"Статья","offset":500,"limit":20})).unwrap()).await.unwrap();
        assert!(found["offset"].as_u64().unwrap() >= 500);
    }
    #[tokio::test]
    async fn quota_failure_and_expiry_are_explicit_without_returning_huge_payload() {
        let dir = tempdir().unwrap();
        let mut storage = store(dir.path().into(), None);
        storage.settings.max_scope_bytes = 20000;
        let original = "x".repeat(17000);
        let first: Value = serde_json::from_str(
            &storage
                .bound("tool".into(), "{}".into(), original.clone())
                .await,
        )
        .unwrap();
        let second = storage.bound("tool".into(), "{}".into(), original).await;
        assert!(second.len() < 5000 && second.contains("quota exhausted"));
        let path = PathBuf::from(first["path"].as_str().unwrap());
        let file = fs::File::options().write(true).open(&path).unwrap();
        file.set_modified(SystemTime::now() - Duration::from_secs(8 * 86400))
            .unwrap();
        let expired = read(&storage, first["artifact_id"].as_str().unwrap(), json!([])).await;
        assert_eq!(expired["error"], "Artifact expired");
        let next = storage
            .bound("tool".into(), "{}".into(), "y".repeat(17000))
            .await;
        assert!(next.contains("\"offloaded\":true"));
        assert!(!path.exists());
        assert!(read(&storage, "../secret", json!([]))
            .await
            .get("error")
            .is_some());
    }
    #[tokio::test]
    async fn disk_failure_does_not_reexecute_tool_and_small_results_are_exact() {
        struct BigTool(Arc<AtomicUsize>);
        impl ToolDyn for BigTool {
            fn name(&self) -> String {
                "big".into()
            }
            fn definition(&self, _: String) -> WasmBoxedFuture<'_, ToolDefinition> {
                Box::pin(async {
                    ToolDefinition {
                        name: "big".into(),
                        description: "test".into(),
                        parameters: json!({}),
                    }
                })
            }
            fn call(&self, _: String) -> WasmBoxedFuture<'_, Result<String, ToolError>> {
                self.0.fetch_add(1, Ordering::SeqCst);
                Box::pin(async { Ok("x".repeat(1000000)) })
            }
        }
        let dir = tempdir().unwrap();
        fs::write(dir.path().join("tool-results"), "blocked").unwrap();
        let storage = store(dir.path().into(), None);
        let calls = Arc::new(AtomicUsize::new(0));
        let tools = storage.wrap(vec![Box::new(BigTool(calls.clone()))]);
        let result = tools[0].call("{}".into()).await.unwrap();
        assert!(result.len() < 5000 && result.contains("artifact_error"));
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        assert_eq!(
            storage
                .bound("tool".into(), "{}".into(), "  exact result\n".into())
                .await,
            "  exact result\n"
        );
    }
    #[tokio::test]
    async fn owner_records_stay_private_and_failed_report_storage_is_not_acknowledged() {
        let dir = tempdir().unwrap();
        let mut owner = Artifacts::new(
            dir.path().into(),
            &("telegram".into(), "room".into()),
            true,
            None,
            Settings::default(),
        );
        let output = json!({"run_id":"child-a","result":{"answer":"x".repeat(20000)}}).to_string();
        let saved: Value = serde_json::from_str(
            &owner
                .bound("subagent".into(), "{}".into(), output.clone())
                .await,
        )
        .unwrap();
        assert!(!saved["result"].is_null());
        let id = saved["artifact_id"].as_str().unwrap();
        assert_eq!(
            read(&store(dir.path().into(), None), id, json!([])).await["error"],
            "Artifact requires owner authority"
        );
        assert!(read(&owner, id, json!([])).await.get("error").is_none());
        owner.settings.max_file_bytes = 1024;
        let failed: Value =
            serde_json::from_str(&owner.bound("subagent".into(), "{}".into(), output).await)
                .unwrap();
        assert!(failed.get("artifact_error").is_some());
        assert!(
            failed["result"].is_null(),
            "unreadable report must remain discoverable by its parent"
        );
    }
}
