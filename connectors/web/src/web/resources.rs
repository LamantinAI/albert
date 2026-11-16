use albert_core::config::{AuthMode, Config};

use super::Panel;
use crate::api::{
    errors::ApiError, errors::ApiResult, host::HostResponse, models::ModelUsageSummary,
    models::ModelUsageView, models::SubscriptionLimitsView, models::SubscriptionWindow,
    principal::Principal,
};

impl Panel {
    pub(super) async fn resource_usage(&self, caller: &Principal) -> ApiResult<HostResponse> {
        self.auth.validate_principal(caller).await?.owner_permit()?;
        let summary = self
            .runtime
            .sessions
            .usage()
            .today()
            .await
            .map_err(|_| ApiError::unavailable("Usage could not be loaded."))?;
        Ok(HostResponse::ModelUsage(ModelUsageSummary {
            tracking_started_at: summary.tracking_started_at,
            period_started_at: summary.period_started_at,
            pending_records: summary.pending_records,
            lost_records: summary.lost_records,
            models: summary
                .models
                .into_iter()
                .map(|m| ModelUsageView {
                    model_id: m.model_id,
                    model: m.model,
                    provider: m.provider,
                    input_tokens: m.input_tokens,
                    output_tokens: m.output_tokens,
                    cached_input_tokens: m.cached_input_tokens,
                    total_tokens: m.total_tokens,
                    reported_calls: m.reported_calls,
                    unreported_calls: m.unreported_calls,
                })
                .collect(),
        }))
    }
    pub(super) async fn resource_limits(&self, caller: &Principal) -> ApiResult<HostResponse> {
        self.auth.validate_principal(caller).await?.owner_permit()?;
        let config = Config::load_from(&self.runtime.path)
            .map_err(|_| ApiError::unavailable("Configuration could not be loaded."))?;
        let base = config
            .models
            .as_ref()
            .and_then(|pool| {
                pool.models
                    .iter()
                    .find(|model| model.provider == AuthMode::Subscription)
            })
            .and_then(|model| model.base_url.as_deref())
            .unwrap_or(&config.subscription_base_url);
        let snapshot = self
            .runtime
            .subscription_limits
            .get(&self.runtime.subscription, base)
            .await;
        self.auth.validate_principal(caller).await?.owner_permit()?;
        Ok(HostResponse::SubscriptionLimits(SubscriptionLimitsView {
            state: snapshot.state,
            observed_at: snapshot.observed_at,
            windows: snapshot
                .windows
                .into_iter()
                .map(|w| SubscriptionWindow {
                    bucket: w.bucket,
                    duration_seconds: w.duration_seconds,
                    remaining_percent: w.remaining_percent,
                    resets_at: w.resets_at,
                })
                .collect(),
        }))
    }
}
