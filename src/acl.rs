//! Owner-only ACL admin: `/allow <id>`, `/deny <id>`, `/allowed`. A deterministic
//! reflex — security stays out of the LLM. Authorization is checked here against
//! the incoming message's role (the Telegram connector stamps it), then the
//! matching `octo.telegram.*` control command is dispatched; the connector trusts
//! this upstream gate.

use std::time::Duration;

use octo_core::{CogitatorContext, ConnectorId, Envelope, EventKind};
use serde_json::{json, Value};
use tracing::warn;

/// Handle an ACL admin command from `text`. Returns the user-facing reply if it is
/// one of the commands, else `None`. `source` is the cogitator's own connector id.
pub async fn command(
    source: &ConnectorId,
    text: &str,
    incoming: &Envelope,
    ctx: &CogitatorContext,
) -> Option<String> {
    let trimmed = text.trim();
    let (cmd, arg) = match trimmed.split_once(char::is_whitespace) {
        Some((c, a)) => (c, a.trim()),
        None => (trimmed, ""),
    };
    let kind = match cmd {
        "/allow" => "octo.telegram.allow_chat",
        "/deny" => "octo.telegram.remove_chat",
        "/allowed" => "octo.telegram.list_chats",
        _ => return None,
    };
    if !is_owner(incoming) {
        warn!(source = %incoming.source, "acl command from non-owner refused");
        return Some("Only the owner can manage the access list.".to_string());
    }
    let payload = match cmd {
        "/allow" | "/deny" => match arg.parse::<i64>() {
            Ok(id) => json!({ "chat_id": id, "role": "trusted" }),
            Err(_) => return Some(format!("Usage: {cmd} <chat_id>")),
        },
        _ => json!({}),
    };
    let req = Envelope::new(source.clone(), EventKind::new(kind), payload)
        .with_target(ConnectorId::new("telegram"));
    match ctx.publish_and_await_response(req, Duration::from_secs(5)).await {
        Ok(resp) => Some(format_result(cmd, resp.payload_as::<Value>())),
        Err(e) => Some(format!("Command failed: {e}")),
    }
}

/// Whether an incoming message came from an owner-trust channel. The Telegram
/// connector stamps `role = "owner"` on the owner's chat metadata.
pub(crate) fn is_owner(env: &Envelope) -> bool {
    env.channel_metadata
        .as_ref()
        .and_then(|m| m.tags.get("role"))
        .map(|r| r.as_str() == "owner")
        .unwrap_or(false)
}

/// Render an `octo.telegram.*.result` payload into a user-facing reply.
fn format_result(cmd: &str, payload: Option<&Value>) -> String {
    let p = payload.cloned().unwrap_or(Value::Null);
    if p.get("ok").and_then(Value::as_bool) != Some(true) {
        let err = p.get("error").and_then(Value::as_str).unwrap_or("unknown error");
        return format!("Error: {err}");
    }
    match cmd {
        "/allow" => {
            let id = p.get("chat_id").and_then(Value::as_i64).unwrap_or_default();
            if p.get("added").and_then(Value::as_bool) == Some(true) {
                format!("Chat {id} added (trusted).")
            } else {
                format!("Chat {id} was already on the list.")
            }
        }
        "/deny" => {
            let id = p.get("chat_id").and_then(Value::as_i64).unwrap_or_default();
            if p.get("removed").and_then(Value::as_bool) == Some(true) {
                format!("Chat {id} removed.")
            } else {
                format!("Chat {id} wasn't on the list.")
            }
        }
        _ => {
            let chats = p.get("chats").and_then(Value::as_array).cloned().unwrap_or_default();
            if chats.is_empty() {
                return "The access list is empty.".to_string();
            }
            let lines: Vec<String> = chats
                .iter()
                .map(|c| {
                    let id = c.get("chat_id").and_then(Value::as_i64).unwrap_or_default();
                    let role = c.get("role").and_then(Value::as_str).unwrap_or("?");
                    format!("- {id} — {role}")
                })
                .collect();
            format!("Access list:\n{}", lines.join("\n"))
        }
    }
}
