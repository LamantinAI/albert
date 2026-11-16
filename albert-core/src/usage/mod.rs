//! Provider-reported accounting, separate from context estimates and quotas.
mod store;

use std::sync::{atomic::AtomicU64, atomic::AtomicUsize, atomic::Ordering, Arc};

use chrono::Utc;
use rand::random;
use rig::completion::Usage;
use serde::{Deserialize, Serialize};
use sqlx::SqlitePool;
use tokio::spawn;
use tracing::warn;

use crate::{config::AuthMode, db::Database, models::ModelSpec};

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ModelUsage {
    pub model_id: String,
    pub model: String,
    pub provider: String,
    pub input_tokens: Option<i64>,
    pub output_tokens: Option<i64>,
    pub cached_input_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub reported_calls: i64,
    pub unreported_calls: i64,
}
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct UsageSummary {
    pub tracking_started_at: String,
    pub period_started_at: String,
    pub models: Vec<ModelUsage>,
    pub pending_records: usize,
    pub lost_records: u64,
}
#[derive(Clone)]
pub struct UsageLedger {
    pool: SqlitePool,
    pending: Arc<AtomicUsize>,
    lost: Arc<AtomicU64>,
}
#[derive(Clone)]
pub(crate) struct UsageRecorder {
    ledger: UsageLedger,
    model_id: String,
    model: String,
    provider: String,
    origin: &'static str,
}
impl UsageLedger {
    pub fn new(database: &Database) -> Self {
        Self {
            pool: database.pool().clone(),
            pending: Arc::new(AtomicUsize::new(0)),
            lost: Arc::new(AtomicU64::new(0)),
        }
    }
    pub(crate) fn recorder(&self, model: &ModelSpec, origin: &'static str) -> UsageRecorder {
        UsageRecorder {
            ledger: self.clone(),
            model_id: model.id.clone(),
            model: model.model.clone(),
            provider: match model.provider {
                AuthMode::Subscription => "subscription".into(),
                AuthMode::ApiKey => "api".into(),
            },
            origin,
        }
    }
}
impl UsageRecorder {
    pub fn record(&self, id: String, usage: Option<Usage>) {
        if self.ledger.pending.fetch_add(1, Ordering::AcqRel) >= 64 {
            self.ledger.pending.fetch_sub(1, Ordering::AcqRel);
            self.ledger.lost.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let recorder = self.clone();
        spawn(async move {
            if recorder
                .insert(&id, &Utc::now().to_rfc3339(), usage)
                .await
                .is_err()
            {
                recorder.ledger.lost.fetch_add(1, Ordering::Relaxed);
                warn!("model usage record could not be retained");
            }
            recorder.ledger.pending.fetch_sub(1, Ordering::AcqRel);
        });
    }
    pub fn invocation() -> String {
        format!("{:032x}", random::<u128>())
    }
}

pub(crate) struct UsageInvocation {
    recorder: UsageRecorder,
    id: Option<String>,
}
impl UsageRecorder {
    pub fn begin(&self) -> UsageInvocation {
        UsageInvocation {
            recorder: self.clone(),
            id: Some(Self::invocation()),
        }
    }
}
impl UsageInvocation {
    pub fn complete(mut self, usage: Option<Usage>) {
        if let Some(id) = self.id.take() {
            self.recorder.record(id, usage);
        }
    }
}
impl Drop for UsageInvocation {
    fn drop(&mut self) {
        if let Some(id) = self.id.take() {
            self.recorder.record(id, None);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{UsageLedger, UsageRecorder};
    use crate::db::Database;
    use chrono::Utc;
    use rig::completion::Usage;
    use sqlx::sqlite::SqlitePoolOptions;

    #[tokio::test]
    async fn accounting_preserves_unknown_zero_and_deduplicates_invocations() {
        let db = Database::from_pool(
            SqlitePoolOptions::new()
                .max_connections(1)
                .connect("sqlite::memory:")
                .await
                .unwrap(),
        )
        .await
        .unwrap();
        let ledger = UsageLedger::new(&db);
        let recorder = UsageRecorder {
            ledger: ledger.clone(),
            model_id: "operator".into(),
            model: "test-model".into(),
            provider: "api".into(),
            origin: "chat",
        };
        let at = Utc::now().to_rfc3339();
        recorder.insert("unknown", &at, None).await.unwrap();
        let unknown = ledger.today().await.unwrap().models.remove(0);
        assert_eq!(unknown.total_tokens, None);
        assert_eq!(unknown.unreported_calls, 1);
        recorder
            .insert("zero", &at, Some(Usage::new()))
            .await
            .unwrap();
        let zero = ledger.today().await.unwrap().models.remove(0);
        assert_eq!(zero.total_tokens, Some(0));
        assert_eq!(zero.reported_calls, 1);
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            total_tokens: 120,
            cached_input_tokens: 80,
            cache_creation_input_tokens: 0,
        };
        recorder.insert("reported", &at, Some(usage)).await.unwrap();
        recorder.insert("reported", &at, Some(usage)).await.unwrap();
        let row = ledger.today().await.unwrap().models.remove(0);
        assert_eq!(row.input_tokens, Some(100));
        assert_eq!(row.total_tokens, Some(120));
        assert_eq!(row.cached_input_tokens, Some(80));
        assert_eq!(row.reported_calls, 2);
        assert_eq!(row.unreported_calls, 1);
    }
}
