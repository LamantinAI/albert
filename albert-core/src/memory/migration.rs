//! Application migration v1: materialise Albert's initiative, without importing
//! conversations or touching an existing initiative. Kaeru owns DB migrations.

use kaeru_rig::{KaeruMemory, NoArgs};
use rig::tool::Tool;
use rmcp::model::CallToolResult;
use serde_json::{from_value, json, Value};

use super::{MemoryError, INITIATIVE};

pub const NAME: &str = "albert";
pub const BODY: &str = "Альберт — персональный ассистент на Octo с памятью kaeru. Инициатива albert — его память по умолчанию: контекст общения, факты и задачи пользователя.";

/// Both backends use the same ordinary cite; kaeru attaches its initiative
/// through the existing capture path. No private schema or separate registry.
pub fn seed() -> Value {
    json!({ "name": NAME, "body": BODY, "initiative": INITIATIVE })
}

pub async fn embedded(memory: &KaeruMemory) -> Result<(), MemoryError> {
    if rig_has_initiative(memory).await? {
        return Ok(());
    }
    let args = from_value(seed()).expect("bootstrap conforms to the kaeru cite schema");
    let result = memory
        .cite()
        .call(args)
        .await
        .expect("rig tool errors are returned as data");
    if result.get("saved").and_then(Value::as_bool) != Some(true) {
        return Err(MemoryError::Embedded(result.to_string()));
    }
    if !rig_has_initiative(memory).await? {
        return Err(MemoryError::Embedded(
            "bootstrap did not create the albert initiative".into(),
        ));
    }
    Ok(())
}

async fn rig_has_initiative(memory: &KaeruMemory) -> Result<bool, MemoryError> {
    let result = memory
        .initiatives()
        .call(NoArgs {})
        .await
        .expect("rig tool errors are returned as data");
    let names = result
        .get("initiatives")
        .and_then(Value::as_array)
        .ok_or_else(|| MemoryError::Embedded(result.to_string()))?;
    Ok(names.iter().any(|name| name.as_str() == Some(INITIATIVE)))
}

/// Kaeru currently returns this read as text. Reject unknown formats instead of
/// treating a server error or truncated reply as an empty memory and writing.
pub fn has_initiative(result: &CallToolResult) -> Result<bool, MemoryError> {
    if result.is_error == Some(true) {
        return Err(MemoryError::Tool("initiatives failed".into()));
    }
    let text = result
        .content
        .iter()
        .filter_map(|c| c.raw.as_text())
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join("\n");
    let text = text.trim();
    if text == "(no initiatives yet — pass `initiative` on a mutation to register one)" {
        return Ok(false);
    }
    let mut lines = text.lines();
    let count = lines
        .next()
        .and_then(|s| s.strip_prefix("initiatives ("))
        .and_then(|s| s.strip_suffix("):"))
        .and_then(|s| s.parse::<usize>().ok())
        .ok_or_else(|| MemoryError::Protocol("unrecognised initiatives response".into()))?;
    let names: Vec<_> = lines
        .filter(|s| !s.trim().is_empty())
        .map(|s| s.trim().strip_prefix("- "))
        .collect::<Option<Vec<_>>>()
        .ok_or_else(|| MemoryError::Protocol("malformed initiatives list".into()))?;
    if names.len() != count {
        return Err(MemoryError::Protocol("incomplete initiatives list".into()));
    }
    Ok(names.contains(&INITIATIVE))
}
