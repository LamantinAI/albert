use axum::http::StatusCode;
use toml::{from_str, Table};

use super::{redact, ControlService};
use crate::api::{errors::ApiError, errors::ApiResult, models::ConfigWrite};

impl ControlService {
    /// Callers hold the mutation gate through validation and the final write.
    pub(in crate::service) fn validate_agent_candidate(
        &self,
        content: &str,
        revision: Option<&str>,
    ) -> ApiResult<()> {
        if let Some(revision) = revision {
            self.check_agent_candidate(content, revision)?;
        }
        let validation = self.validate(&ConfigWrite {
            path: "albert".into(),
            content: content.into(),
            revision: revision.map(str::to_owned),
        });
        if validation.valid {
            Ok(())
        } else {
            Err(ApiError::invalid(validation.issues.join(" ")))
        }
    }
    /// Pre-merge syntax, authority and revision checks; the complete merged
    /// configuration is validated after the dedicated Memory draft is overlaid.
    pub(in crate::service) fn check_agent_candidate(
        &self,
        content: &str,
        revision: &str,
    ) -> ApiResult<()> {
        if content.len() > 1024 * 1024 {
            return Err(ApiError::invalid("Configuration exceeds 1 MiB."));
        }
        from_str::<Table>(content)
            .map_err(|_| ApiError::invalid("Correct the Agent document syntax before saving."))?;
        let current = self.document("albert")?;
        if current.revision != revision {
            return Err(ApiError::new(
                StatusCode::PRECONDITION_FAILED,
                "revision_changed",
                "Agent settings changed. Review the current settings before saving.",
            ));
        }
        if current.redacted || content.contains("[REDACTED]") || redact(content.to_owned()).1 {
            return Err(ApiError::invalid(
                "Use named credential references instead of inline secrets in the Agent document.",
            ));
        }
        Ok(())
    }
}
