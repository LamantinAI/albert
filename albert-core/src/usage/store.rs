use std::sync::atomic::Ordering;

use chrono::Utc;
use rig::completion::Usage;
use sqlx::{query, query_as, query_scalar};

use super::{ModelUsage, UsageLedger, UsageRecorder, UsageSummary};
use crate::db::error::DatabaseResult;

impl UsageRecorder {
    pub(super) async fn insert(
        &self,
        id: &str,
        at: &str,
        usage: Option<Usage>,
    ) -> DatabaseResult<()> {
        let number = |n: u64| i64::try_from(n).ok();
        query("INSERT OR IGNORE INTO albert_model_usage (invocation_id,recorded_at,model_id,model,provider,origin,input_tokens,output_tokens,cached_input_tokens,total_tokens) VALUES (?,?,?,?,?,?,?,?,?,?)")
            .bind(id).bind(at).bind(&self.model_id).bind(&self.model).bind(&self.provider).bind(self.origin)
            .bind(usage.and_then(|u| number(u.input_tokens)))
            .bind(usage.and_then(|u| number(u.output_tokens)))
            .bind(usage.and_then(|u| number(u.cached_input_tokens)))
            .bind(usage.and_then(|u| number(if u.total_tokens>0 {u.total_tokens}else{u.input_tokens.saturating_add(u.output_tokens)})))
            .execute(&self.ledger.pool).await?;
        Ok(())
    }
}
impl UsageLedger {
    pub async fn today(&self) -> DatabaseResult<UsageSummary> {
        let start = format!("{}T00:00:00+00:00", Utc::now().date_naive());
        let tracking_started_at: String =
            query_scalar("SELECT started_at FROM albert_usage_tracking WHERE id=1")
                .fetch_one(&self.pool)
                .await?;
        let rows: Vec<(String,String,String,Option<i64>,Option<i64>,Option<i64>,Option<i64>,i64,i64)> = query_as(
            "SELECT model_id,model,provider,SUM(input_tokens),SUM(output_tokens),SUM(cached_input_tokens),SUM(total_tokens),COUNT(input_tokens),COUNT(*)-COUNT(input_tokens) FROM albert_model_usage WHERE recorded_at>=? GROUP BY model_id,model,provider ORDER BY model_id,model,provider"
        ).bind(&start).fetch_all(&self.pool).await?;
        Ok(UsageSummary {
            tracking_started_at,
            period_started_at: start,
            models: rows
                .into_iter()
                .map(|r| ModelUsage {
                    model_id: r.0,
                    model: r.1,
                    provider: r.2,
                    input_tokens: r.3,
                    output_tokens: r.4,
                    cached_input_tokens: r.5,
                    total_tokens: r.6,
                    reported_calls: r.7,
                    unreported_calls: r.8,
                })
                .collect(),
            pending_records: self.pending.load(Ordering::Acquire),
            lost_records: self.lost.load(Ordering::Relaxed),
        })
    }
}
