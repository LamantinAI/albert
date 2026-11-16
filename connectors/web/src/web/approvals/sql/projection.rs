use chrono::Utc;
use octo_approval::{ApprovalAction, ApprovalPresentation};
use serde::Deserialize;
use serde_json::{from_value, Value};

use super::SqlAdapter;
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::ApprovalState,
    web::approvals::native::NativeRecord,
};

#[derive(Default, Deserialize)]
#[serde(default)]
struct Summary {
    resource: String,
    action: ApprovalAction,
    objects: Vec<String>,
}
impl SqlAdapter {
    pub(super) fn project(
        executor: &str,
        principal: &str,
        item: &Value,
    ) -> ApiResult<NativeRecord> {
        let summary: Summary =
            from_value(item["summary"].clone()).map_err(|_| ApiError::internal())?;
        let state = match item["status"].as_str() {
            Some("pending" | "approval_required") => ApprovalState::Pending,
            Some("approved" | "claimed") => ApprovalState::Deciding,
            Some("committed") => ApprovalState::Committed,
            Some("rejected") => ApprovalState::Rejected,
            Some("cancelled") => ApprovalState::Cancelled,
            Some("expired") => ApprovalState::Expired,
            Some("rolled_back") => ApprovalState::RolledBack,
            _ => ApprovalState::Unknown,
        };
        let operation = item["operation_id"]
            .as_str()
            .ok_or_else(ApiError::internal)?
            .to_owned();
        let hash = item["operation_hash"]
            .as_str()
            .ok_or_else(ApiError::internal)?
            .to_owned();
        let expires = item["expires_at"].as_i64().ok_or_else(ApiError::internal)?;
        let scope = if item["origin_scope"].is_null() {
            None
        } else {
            Some(from_value(item["origin_scope"].clone()).map_err(|_| ApiError::internal())?)
        };
        Ok(NativeRecord {
            adapter: "sql".into(),
            executor: executor.into(),
            operation,
            operation_hash: hash,
            recipient_principal: principal.into(),
            origin_scope: scope,
            decision_connector: item["decision_connector"]
                .as_str()
                .ok_or_else(ApiError::internal)?
                .into(),
            private_destination: item["private_destination"]
                .as_str()
                .ok_or_else(ApiError::internal)?
                .into(),
            state,
            title: title(summary.action, &summary.resource),
            action: action(summary.action).into(),
            effects: effects(&summary.objects),
            details: item.get("details").cloned(),
            expires_at: expires,
            can_decide: state == ApprovalState::Pending
                && expires > Utc::now().timestamp()
                && item["can_decide"] != false,
        })
    }
    pub(super) fn notice(value: &ApprovalPresentation, connector: &str) -> NativeRecord {
        NativeRecord {
            adapter: "sql".into(),
            executor: value.executor.clone(),
            operation: value.operation_id.clone(),
            operation_hash: value.operation_hash.clone(),
            recipient_principal: value.recipient_principal.clone(),
            origin_scope: value.origin_scope.clone(),
            decision_connector: connector.into(),
            private_destination: value.private_destination.clone(),
            state: ApprovalState::Pending,
            title: title(value.action, &value.resource),
            action: action(value.action).into(),
            effects: effects(&value.objects),
            details: None,
            expires_at: value.expires_at,
            can_decide: value.expires_at > Utc::now().timestamp(),
        }
    }
}
fn title(kind: ApprovalAction, resource: &str) -> String {
    if resource.is_empty() {
        format!("{}?", action(kind))
    } else {
        format!("{} in “{}”?", action(kind), display(resource))
    }
}
fn action(kind: ApprovalAction) -> &'static str {
    match kind {
        ApprovalAction::DeleteStore => "Delete store",
        ApprovalAction::DeleteRecords => "Delete records",
        ApprovalAction::DropObjects => "Drop tables or views",
        ApprovalAction::DropColumns => "Drop columns",
        ApprovalAction::ReplaceRecords => "Replace records",
        ApprovalAction::UpdateRecords => "Update records",
        ApprovalAction::ChangeData => "Change stored data",
    }
}
fn effects(objects: &[String]) -> Vec<String> {
    let mut effects=vec!["This operation changes persisted data. Committed changes cannot be undone by the connector.".into()];
    if !objects.is_empty() {
        effects.push(format!(
            "Affected objects: {}",
            objects
                .iter()
                .map(|name| display(name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    effects
}

fn display(text: &str) -> String {
    text.chars()
        .filter(|c| {
            !c.is_control() && !matches!(*c,'\u{202a}'..='\u{202e}'|'\u{2066}'..='\u{2069}')
        })
        .collect()
}
