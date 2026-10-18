//! Integration of real SQL storage with root and child model-tool dispatch.
#[cfg(test)]
mod tests {
    use std::{sync::Arc, time::Duration};

    use octo_approval::{Access, Ingress, Origin};
    use octo_connector_sql::{Database, Limits};
    use octo_core::{
        CogitatorContext, ConnectorCapabilities, ConnectorId, ConnectorInfo, Envelope, EventBus,
        EventKind, Filter, SubscribeOptions,
    };
    use rig::completion::Message;
    use serde_json::{from_value, json, to_string, Value};
    use tempfile::{tempdir, TempDir};
    use tokio::{spawn, task::JoinHandle, time::timeout};

    use crate::{
        cogitator::{
            provider_fixture::{setup, Server},
            AlbertCogitator,
        },
        status::StatusFeed,
        subagents::{Args, SubagentTool, Task},
    };

    struct Fixture {
        host: Arc<AlbertCogitator>,
        ctx: CogitatorContext,
        server: Server,
        db: Arc<Database>,
        _dir: TempDir,
        task: JoinHandle<()>,
        ingress: Ingress,
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            self.task.abort();
        }
    }
    impl Fixture {
        async fn new(model: &str) -> Self {
            let (host, ctx, server) = setup(model).await;
            let dir = tempdir().unwrap();
            let db = Database::open(
                dir.path().join("data.db"),
                Limits::default(),
                host.authority.clone(),
                vec![],
            )
            .await
            .unwrap();
            let ingress = host.authority.issue(
                Origin {
                    principal: "test-owner".into(),
                    connector: "telegram".into(),
                    conversation: "1".into(),
                    private_destination: "1".into(),
                    may_approve: true,
                },
                Access::owner(),
            );
            let ctx = CogitatorContext::new(
                ctx.shutdown.clone(),
                ctx.bus(),
                vec![ConnectorInfo {
                    id: ConnectorId::new("sql"),
                    capabilities: ConnectorCapabilities::bidirectional()
                        .with_accept_kinds([
                            EventKind::new("sql.store.create"),
                            EventKind::new("sql.store.list"),
                        ])
                        .with_description(
                            "sql.store.create {name,purpose,idempotency_key}; sql.store.list {}",
                        ),
                }],
            );
            let mut requests = ctx
                .bus()
                .subscribe(
                    Filter::by_target(ConnectorId::new("sql")),
                    SubscribeOptions::default(),
                )
                .await
                .unwrap();
            let bus = ctx.bus();
            let backend = db.clone();
            let task = spawn(async move {
                while let Some(request) = requests.next().await {
                    let result = match backend.handle(&request).await {
                        Ok(value) => value,
                        Err(error) => json!({"error":error.to_string()}),
                    };
                    bus.publish(
                        Envelope::new(
                            ConnectorId::new("sql"),
                            EventKind::new(format!("{}.result", request.kind)),
                            result,
                        )
                        .with_correlation(request.id),
                    )
                    .await
                    .unwrap();
                }
            });
            Self {
                host,
                ctx,
                server,
                db,
                _dir: dir,
                task,
                ingress,
            }
        }
        async fn call(&self, kind: &str, payload: Value) -> Value {
            let envelope = Envelope::new(ConnectorId::new("host"), EventKind::new(kind), payload)
                .with_target(ConnectorId::new("sql"));
            let proof = self.host.authority.bind(&self.ingress, &envelope).unwrap();
            self.db.handle(&envelope.with_context(proof)).await.unwrap()
        }
    }

    #[tokio::test]
    async fn root_sql_dispatch_requires_native_ingress_and_cannot_use_owner_tags_alone() {
        for authorized in [false, true] {
            let f = Fixture::new("sql-create").await;
            let feed = StatusFeed::silent();
            let answer = timeout(
                Duration::from_secs(5),
                f.host.run_agent_with_ingress(
                    &f.ctx,
                    "1",
                    "test",
                    Message::user("create a data store"),
                    vec![],
                    Some(ConnectorId::new("telegram")),
                    true,
                    feed.clone(),
                    Some("root-sql"),
                    authorized.then(|| f.ingress.clone()),
                ),
            )
            .await
            .unwrap();
            assert_eq!(answer.0, "answer from sql-create");
            assert_eq!(feed.tool_call_count(), 1);
            let stored = f.call("sql.store.list", json!({})).await;
            assert_eq!(
                stored["stores"].as_array().unwrap().len(),
                usize::from(authorized)
            );
            assert_eq!(f.server.requests.lock().unwrap().len(), 2);
        }
    }

    #[tokio::test]
    async fn child_sees_only_explicit_sql_store_grant_and_has_no_create_authority() {
        let f = Fixture::new("sql-list").await;
        let first = f
            .call(
                "sql.store.create",
                json!({"name":"allowed","idempotency_key":"first"}),
            )
            .await;
        let second = f
            .call(
                "sql.store.create",
                json!({"name":"hidden","idempotency_key":"second"}),
            )
            .await;
        let tool = SubagentTool::new(
            Arc::downgrade(&f.host),
            f.ctx.clone(),
            ("telegram".into(), "1".into()),
            "parent-sql".into(),
            true,
        )
        .with_ingress(Some(f.ingress.clone()));
        let task: Task = from_value(
            json!({"task":"list permitted data stores","connectors":["sql"],
            "sql_stores":[first["store_id"]]}),
        )
        .unwrap();
        let started = f
            .host
            .subagent_command(&tool, Args::Spawn { task })
            .await
            .unwrap();
        let result = timeout(
            Duration::from_secs(5),
            f.host.subagent_command(
                &tool,
                Args::Wait {
                    run_id: started["run_id"].as_str().unwrap().into(),
                    seconds: 4,
                },
            ),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(result["result"]["outcome"]["status"], "completed");
        let requests = f.server.requests.lock().unwrap();
        let wire = to_string(&requests[1]["messages"]).unwrap();
        assert!(wire.contains(first["store_id"].as_str().unwrap()));
        assert!(!wire.contains(second["store_id"].as_str().unwrap()));
        drop(requests);
        let child = f
            .ingress
            .attenuate(
                vec![first["store_id"].as_str().unwrap().into()],
                false,
                false,
            )
            .unwrap();
        let envelope = Envelope::new(
            ConnectorId::new("child"),
            EventKind::new("sql.store.create"),
            json!({"name":"forbidden","idempotency_key":"child-create"}),
        )
        .with_target(ConnectorId::new("sql"));
        let proof = f.host.authority.bind(&child, &envelope).unwrap();
        assert!(f.db.handle(&envelope.with_context(proof)).await.is_err());
    }
}
