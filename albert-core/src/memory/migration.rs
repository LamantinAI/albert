//! Materialise Albert's initiative through the canonical memory API. The memory
//! backend owns vault schema upgrades; existing initiative contents stay intact.
use kaeru_core::api::{Response, ResponseData, State, API_VERSION};
use kaeru_rig::KaeruMemory;
use rig::tool::Tool;
use rmcp::model::CallToolResult;
use serde_json::{from_str, from_value, json, Value};

use super::{MemoryError, INITIATIVE};

pub const NAME: &str = "albert";
pub const BODY: &str = "Albert is a personal assistant built on Octo with kaeru memory. The albert initiative is its default memory for conversation context, user facts and tasks.";

pub fn seed() -> Value {
    json!({"name":NAME,"body":BODY,"initiative":INITIATIVE,"kind":"observation",
        "request_id":"albert/initiative-bootstrap/v1"})
}
pub fn initiatives_request() -> Value {
    json!({"filter":INITIATIVE})
}

pub async fn embedded(memory: &KaeruMemory) -> Result<(), MemoryError> {
    let list = memory
        .initiatives()
        .call(initiatives_request())
        .await
        .expect("infallible memory tool");
    if initiative_value(list)? {
        return Ok(());
    }
    let created = memory
        .note()
        .call(seed())
        .await
        .expect("infallible memory tool");
    mutation_value(created)?;
    let verified = memory
        .initiatives()
        .call(initiatives_request())
        .await
        .expect("infallible memory tool");
    if !initiative_value(verified)? {
        return Err(MemoryError::Protocol(
            "bootstrap did not create the albert initiative".into(),
        ));
    }
    Ok(())
}
fn response(value: Value, operation: &str) -> Result<Response, MemoryError> {
    let result: Response = from_value(value)
        .map_err(|_| MemoryError::Protocol("invalid typed memory response".into()))?;
    if result.api_version != API_VERSION
        || result.operation != operation
        || result.is_error()
        || result
            .issues
            .iter()
            .any(|issue| operation != "note" || issue.code != "NEW_INITIATIVE")
    {
        return Err(MemoryError::Protocol(format!(
            "memory {operation} did not complete successfully ({})",
            result
                .issues
                .iter()
                .map(|issue| issue.code.as_str())
                .collect::<Vec<_>>()
                .join(",")
        )));
    }
    Ok(result)
}
pub fn initiative_value(value: Value) -> Result<bool, MemoryError> {
    let result = response(value, "initiatives")?;
    if !matches!(result.state, State::Ok) {
        return Err(MemoryError::Protocol("invalid initiatives state".into()));
    }
    let ResponseData::Initiatives(data) = result.data else {
        return Err(MemoryError::Protocol("invalid initiatives data".into()));
    };
    if data.initiatives.iter().any(|name| name == INITIATIVE) {
        return Ok(true);
    }
    if result.continuation.is_some()
        || result
            .coverage
            .as_ref()
            .is_none_or(|coverage| !coverage.complete || !coverage.omitted.is_empty())
    {
        return Err(MemoryError::Protocol(
            "incomplete initiatives list; bootstrap was not attempted".into(),
        ));
    }
    Ok(false)
}
pub fn mutation_value(value: Value) -> Result<(), MemoryError> {
    let result = response(value, "note")?;
    if !matches!(result.state, State::Applied | State::Unchanged)
        || !matches!(result.data, ResponseData::Mutation(_))
    {
        return Err(MemoryError::Protocol(
            "memory bootstrap was not applied".into(),
        ));
    }
    Ok(())
}
fn content_value(result: &CallToolResult) -> Result<Value, MemoryError> {
    if result.is_error == Some(true) {
        return Err(MemoryError::Tool("memory bootstrap request failed".into()));
    }
    if let Some(value) = &result.structured_content {
        return Ok(value.clone());
    }
    let text = result
        .content
        .iter()
        .filter_map(|content| content.raw.as_text())
        .map(|content| content.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    from_str(&text)
        .map_err(|_| MemoryError::Protocol("memory response must contain typed JSON".into()))
}
pub fn has_initiative(result: &CallToolResult) -> Result<bool, MemoryError> {
    initiative_value(content_value(result)?)
}
pub fn mutation(result: &CallToolResult) -> Result<(), MemoryError> {
    mutation_value(content_value(result)?)
}
