//! Deterministic chat administration. Actor identity comes from connector metadata,
//! never from the message body or the group's own ACL role.

use std::time::Duration;

use octo_core::{CogitatorContext, ConnectorId, Envelope, EventKind};
use serde_json::{json, Value};

use crate::commands::parse;

pub const BOOTSTRAP_COMMANDS: [&str; 5] = ["allow", "deny", "allowed", "groupmode", "chatinfo"];

pub fn tag<'a>(env: &'a Envelope, name: &str) -> Option<&'a str> {
    env.channel_metadata
        .as_ref()?
        .tags
        .get(name)
        .map(String::as_str)
}

pub(crate) fn is_owner(env: &Envelope) -> bool {
    tag(env, "role") == Some("owner")
}

pub(crate) fn is_acl_admin(env: &Envelope) -> bool {
    is_owner(env) || tag(env, "acl_admin") == Some("true")
}

pub fn is_group(env: &Envelope) -> bool {
    matches!(tag(env, "chat_type"), Some("group" | "supergroup"))
}

/// Quiet group messages do not start/cancel a turn, transcribe audio or call an LLM.
pub fn should_respond(env: &Envelope) -> bool {
    if is_group(env) && tag(env, "addressed") != Some("true") {
        return false;
    }
    if tag(env, "bootstrap_only") == Some("true") {
        return tag(env, "command_text")
            .or_else(|| env.payload_as::<String>().map(String::as_str))
            .and_then(parse)
            .is_some_and(|cmd| BOOTSTRAP_COMMANDS.contains(&cmd.name.as_str()));
    }
    true
}

pub fn source_info(env: &Envelope) -> String {
    let chat = env
        .channel
        .as_ref()
        .map(|id| id.as_str())
        .unwrap_or("unknown");
    let mut out = format!(
        "Connector: {}\nChat ID: {chat}\nChat type: {}",
        env.source,
        tag(env, "chat_type").unwrap_or("unknown")
    );
    if let Some(id) = tag(env, "sender_id") {
        out.push_str(&format!("\nAuthor user ID: {id}"));
    } else {
        out.push_str("\nAuthor user ID: unavailable (anonymous or channel sender)");
    }
    if let Some(name) = tag(env, "sender_username") {
        out.push_str(&format!("\nAuthor: @{name}"));
    }
    if let Some(id) = tag(env, "sender_chat_id") {
        out.push_str(&format!("\nSender chat ID: {id}"));
    }
    if let Some(mode) = tag(env, "group_mode") {
        out.push_str(&format!("\nGroup mode: {mode}"));
    }
    out
}

fn request(text: &str, env: &Envelope) -> Option<Result<(&'static str, Value), String>> {
    let invocation = parse(text)?;
    let name = invocation.name.as_str();
    if !BOOTSTRAP_COMMANDS.contains(&name) || name == "chatinfo" {
        return None;
    }
    if !is_acl_admin(env) || (name == "groupmode" && !is_owner(env)) {
        return Some(Err(if name == "groupmode" {
            "Only the owner can change group mode."
        } else {
            "Only the owner or an ACL admin can manage access."
        }
        .into()));
    }
    let current_group = || {
        env.channel
            .as_ref()
            .and_then(|id| id.as_str().parse::<i64>().ok())
            .filter(|_| is_group(env))
    };
    Some(match name {
        "allowed" if invocation.args.is_empty() => Ok(("octo.telegram.list_chats", json!({}))),
        "allow" | "deny" => {
            let id = if invocation.args.is_empty() {
                current_group()
            } else {
                invocation.args.parse::<i64>().ok()
            };
            match id.filter(|id| *id != 0) {
                Some(id) => Ok((
                    if name == "allow" {
                        "octo.telegram.allow_chat"
                    } else {
                        "octo.telegram.remove_chat"
                    },
                    json!({"chat_id":id,"role":"trusted"}),
                )),
                None => Err(format!(
                    "Usage: /{name} <chat_id>, or /{name} in the group itself."
                )),
            }
        }
        "groupmode" => {
            let args: Vec<_> = invocation.args.split_whitespace().collect();
            let id = match args.as_slice() {
                [_] => current_group(),
                [_, id] => id.parse::<i64>().ok(),
                _ => None,
            };
            match (args.first().copied(), id.filter(|id| *id < 0)) {
                (Some(mode @ ("all" | "allowed")), Some(id)) => Ok((
                    "octo.telegram.group_mode",
                    json!({"chat_id":id,"mode":mode}),
                )),
                _ => Err(
                    "Usage: /groupmode all|allowed [group_chat_id]. Omit the ID inside the group."
                        .into(),
                ),
            }
        }
        _ => Err("Usage: /allowed".into()),
    })
}

pub async fn command(
    source: &ConnectorId,
    text: &str,
    incoming: &Envelope,
    ctx: &CogitatorContext,
) -> Option<String> {
    if parse(text).is_some_and(|cmd| cmd.name == "chatinfo") {
        return Some(source_info(incoming));
    }
    let (kind, payload) = match request(text, incoming)? {
        Ok(value) => value,
        Err(error) => return Some(error),
    };
    // Dispatch to the actual channel connector, which may have a custom instance id.
    let mut req = Envelope::new(source.clone(), EventKind::new(kind), payload)
        .with_target(incoming.source.clone());
    if let Some(metadata) = incoming.channel_metadata.clone() {
        req = req.with_channel_metadata(metadata);
    }
    match ctx
        .publish_and_await_response(req, Duration::from_secs(5))
        .await
    {
        Ok(response) => Some(format_result(kind, response.payload_as::<Value>())),
        Err(error) => Some(format!("Command failed: {error}")),
    }
}

fn format_result(kind: &str, payload: Option<&Value>) -> String {
    let p = payload.cloned().unwrap_or(Value::Null);
    if p["ok"].as_bool() != Some(true) {
        return format!("Error: {}", p["error"].as_str().unwrap_or("unknown error"));
    }
    match kind {
        "octo.telegram.group_mode" => format!("Group {}: {}. I will reply only when addressed or given a command.",p["chat_id"],p["mode"].as_str().unwrap_or("?")),
        "octo.telegram.allow_chat" => format!("Chat {} is allowed. Groups default to allowed users only; the owner can use /groupmode all to admit everyone.",p["chat_id"]),
        "octo.telegram.remove_chat" => format!("Chat {} removed.",p["chat_id"]),
        _ => {
            let chats=p["chats"].as_array().cloned().unwrap_or_default();
            if chats.is_empty() { return "The access list is empty.".into(); }
            let lines: Vec<_>=chats.iter().map(|entry| {
                let mode=entry["group_mode"].as_str().map(|mode|format!("; group mode: {mode}")).unwrap_or_default();
                format!("- {} — {}{mode}",entry["chat_id"],entry["role"].as_str().unwrap_or("?"))
            }).collect();
            format!("Access list:\n{}",lines.join("\n"))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octo_core::{ChannelId, ChannelMetadata};

    fn group(role: &str, admin: bool) -> Envelope {
        Envelope::new(
            ConnectorId::new("telegram-work"),
            EventKind::new("chat.message"),
            String::new(),
        )
        .with_channel(ChannelId::new("-42"))
        .with_channel_metadata(
            ChannelMetadata::new()
                .with_tag("chat_type", "supergroup")
                .with_tag("role", role)
                .with_tag("acl_admin", admin.to_string())
                .with_tag("sender_id", "7"),
        )
    }
    #[test]
    fn only_owner_can_open_a_group_and_allow_without_id_uses_current_group() {
        assert!(request("/groupmode all", &group("trusted", true))
            .unwrap()
            .is_err());
        assert!(request("/groupmode all", &group("guest", false))
            .unwrap()
            .is_err());
        assert_eq!(
            request("/groupmode@bot all", &group("owner", true))
                .unwrap()
                .unwrap()
                .1,
            json!({"chat_id":-42,"mode":"all"})
        );
        assert_eq!(
            request("/allow", &group("trusted", true))
                .unwrap()
                .unwrap()
                .1["chat_id"],
            -42
        );
        assert!(request("/allow", &group("guest", false)).unwrap().is_err());
    }
    #[test]
    fn source_report_separates_author_and_room_and_unaddressed_groups_stay_quiet() {
        let env = group("owner", true);
        let text = source_info(&env);
        assert!(text.contains("Chat ID: -42"));
        assert!(text.contains("Author user ID: 7"));
        assert!(!should_respond(&env));
    }
}
