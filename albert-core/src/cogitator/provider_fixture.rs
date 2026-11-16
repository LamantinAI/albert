mod buffered;
use self::buffered::buffered_completion;
mod stream;
use std::{sync::Arc, sync::Mutex};

use axum::{extract::State, response::Response, routing::post, serve, Json, Router};
use octo_core::CogitatorContext;
use serde_json::Value;
use tokio::{net::TcpListener, spawn, task::JoinHandle};

use self::stream::response;
use super::{fixture::fixture, AlbertCogitator};
use crate::{
    config::AuthMode, models::ModelPool, models::ModelSpec, models::PoolConfig, models::RoleProfile,
};
pub struct Server {
    pub requests: Arc<Mutex<Vec<Value>>>,
    task: JoinHandle<()>,
    url: String,
}
impl Drop for Server {
    fn drop(&mut self) {
        self.task.abort();
    }
}

async fn completion(state: State<Arc<Mutex<Vec<Value>>>>, Json(body): Json<Value>) -> Response {
    let streaming = body["stream"] == true;
    let (status, Json(value)) = buffered_completion(state, Json(body)).await;
    response(status, value, streaming)
}

pub(super) async fn setup(first: &str) -> (Arc<AlbertCogitator>, CogitatorContext, Server) {
    let requests = Arc::new(Mutex::new(Vec::new()));
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let app = Router::new()
        .route("/chat/completions", post(completion))
        .with_state(requests.clone());
    let task = spawn(async move {
        serve(listener, app).await.unwrap();
    });
    let server = Server {
        requests,
        task,
        url,
    };
    let (mut agent, ctx, _) = fixture();
    let me = Arc::get_mut(&mut agent).unwrap();
    me.config.api_key = Some("local-test-key".into());
    me.config.models = Some(PoolConfig {
        default: first.into(),
        roles: vec![RoleProfile {
            id: "test_models".into(),
            description: "Explicit test routing".into(),
            models: if first == "healthy" {
                vec![first.into()]
            } else {
                vec![first.into(), "healthy".into()]
            },
            reasoning_effort: None,
        }],
        max_attempts: 3,
        retries_per_model: 0,
        retry_delay_ms: 0,
        models: (if first == "healthy" {
            vec![first]
        } else {
            vec![first, "healthy"]
        })
        .into_iter()
        .map(|id| ModelSpec {
            id: id.into(),
            model: id.into(),
            context_window: None,
            provider: AuthMode::ApiKey,
            base_url: Some(server.url.clone()),
            api_key_env: None,
            vision: true,
            tools: true,
            reasoning: None,
            reasoning_effort: None,
            request_timeout_ms: match id {
                "hanging" => 20,
                "account-hold" => 10000,
                _ => 2000,
            },
        })
        .collect(),
    });
    me.models = ModelPool::new(&me.config);
    (agent, ctx, server)
}
