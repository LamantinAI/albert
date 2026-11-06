//! Semantic presentation metadata resolved only from the installed tool surface.
use serde_json::{from_str, Value};

use super::{domain::ProgressActivity, domain::ProgressItem, progress::item, StatusFeed};

impl StatusFeed {
    pub(super) fn tool_progress_item(
        &self,
        id: String,
        name: &str,
        arguments: &str,
        state: &str,
    ) -> ProgressItem {
        let parameters = matches!(name, "dispatch_to_connector" | "scratchpad_goal")
            .then(|| from_str::<Value>(arguments).ok())
            .flatten();
        let label = self.progress_tool_label(name, parameters.as_ref());
        let mut progress = item(id, "tool", &label, state);
        progress.activity = self.tool_activity(name, parameters.as_ref());
        progress
    }

    fn tool_activity(&self, name: &str, parameters: Option<&Value>) -> Option<ProgressActivity> {
        if !self.tool_known(name) {
            return None;
        }
        if self.memory_tool_known(name) {
            return Some(ProgressActivity::Memory);
        }
        Some(match name {
            "scratchpad_goal" | "scratchpad_step" | "scratchpad_mark" | "scratchpad_note"
            | "scratchpad_clear" | "scratchpad_plan" => ProgressActivity::Scratchpad,
            "skill_list" | "skill_search" | "skill_apply" | "skill_file" => ProgressActivity::Skill,
            "read" | "write" | "edit" | "list" | "glob" | "grep" | "artifact" | "send_file" => {
                ProgressActivity::Files
            }
            "config_read"
            | "config_list"
            | "config_write"
            | "config_edit"
            | "config_set_secret"
            | "config_list_secrets"
            | "restart" => ProgressActivity::Configuration,
            "model_select" => ProgressActivity::Model,
            "subagent" => ProgressActivity::Subagent,
            "session_messaging" | "report_progress" => ProgressActivity::Sessions,
            "connector_search" => ProgressActivity::Connector,
            "dispatch_to_connector" => {
                let parameters = parameters?;
                let target = parameters.get("target")?.as_str()?;
                let kind = parameters.get("kind")?.as_str()?;
                if !self.connector_accepts(target, kind) {
                    return None;
                }
                connector_activity(kind)
            }
            _ => return None,
        })
    }
}

fn connector_activity(kind: &str) -> ProgressActivity {
    match kind {
        "search.web" => ProgressActivity::Search,
        "browser.fetch" => ProgressActivity::Browser,
        "storage.put" | "storage.get" | "storage.list" | "storage.delete" | "storage.promote"
        | "storage.checkout" => ProgressActivity::Files,
        "forkd.run" => ProgressActivity::Shell,
        "sql.store.create"
        | "sql.store.list"
        | "sql.store.search"
        | "sql.store.describe"
        | "sql.store.update_metadata"
        | "sql.store.archive"
        | "sql.store.drop"
        | "sql.execute"
        | "sql.schema.apply"
        | "sql.operation.status"
        | "sql.approval.list" => ProgressActivity::Sql,
        "calendar.list_events"
        | "calendar.create_event"
        | "calendar.delete_event"
        | "octo.scheduler.add_alarm"
        | "octo.scheduler.cancel_alarm"
        | "octo.scheduler.list_alarms" => ProgressActivity::Calendar,
        "mail.cmd.list" | "mail.cmd.read" | "mail.cmd.send" | "mail.cmd.reply" => {
            ProgressActivity::Mail
        }
        "commons.cmd.search" | "imagegen.run" => ProgressActivity::Image,
        "transcribe.run" => ProgressActivity::AudioInput,
        "speak.run" => ProgressActivity::AudioOutput,
        "chat.reply"
        | "chat.send_file"
        | "chat.typing"
        | "chat.status"
        | "chat.turn.end"
        | "chat.set_commands"
        | "octo.telegram.allow_chat"
        | "octo.telegram.remove_chat"
        | "octo.telegram.list_chats"
        | "octo.telegram.group_mode" => ProgressActivity::Sessions,
        _ => ProgressActivity::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use octo_core::{ConnectorCapabilities, ConnectorId, ConnectorInfo, EventKind};
    use serde_json::{json, to_string};

    use super::{connector_activity, ProgressActivity};
    use crate::status::StatusFeed;

    fn connector(id: &str, accepts: &[&str], advertised: bool) -> ConnectorInfo {
        let capabilities = ConnectorCapabilities::bidirectional()
            .with_accept_kinds(accepts.iter().map(|kind| EventKind::new(*kind)));
        ConnectorInfo {
            id: ConnectorId::new(id),
            capabilities: if advertised {
                capabilities.with_description("A public connector contract")
            } else {
                capabilities
            },
        }
    }

    #[test]
    fn native_tools_require_registration_and_memory_requires_exact_origin() {
        let feed = StatusFeed::silent();
        let cases = [
            ("scratchpad_goal", ProgressActivity::Scratchpad),
            ("skill_apply", ProgressActivity::Skill),
            ("artifact", ProgressActivity::Files),
            ("read", ProgressActivity::Files),
            ("config_set_secret", ProgressActivity::Configuration),
            ("model_select", ProgressActivity::Model),
            ("subagent", ProgressActivity::Subagent),
            ("session_messaging", ProgressActivity::Sessions),
            ("connector_search", ProgressActivity::Connector),
        ];
        feed.register_tool_names(cases.iter().map(|(name, _)| name.to_string()).chain([
            "kaeru_custom_verb".into(),
            "kaeru_impostor".into(),
            "scratchpad_impostor".into(),
        ]));
        feed.register_memory_tool_names(["kaeru_custom_verb".into(), "kaeru_not_granted".into()]);
        for (name, expected) in cases {
            assert_eq!(feed.tool_activity(name, None), Some(expected));
        }
        assert_eq!(
            feed.tool_activity("kaeru_custom_verb", None),
            Some(ProgressActivity::Memory)
        );
        for name in [
            "kaeru_impostor",
            "kaeru_not_granted",
            "scratchpad_impostor",
            "skill_search",
            "unknown",
        ] {
            assert_eq!(feed.tool_activity(name, None), None);
        }
    }

    #[test]
    fn dispatch_requires_an_advertised_target_and_an_accepted_concrete_operation() {
        let feed = StatusFeed::silent();
        feed.register_tool_names(["dispatch_to_connector".into()]);
        feed.register_connectors(&[
            connector("renamed-browser", &["browser.fetch"], true),
            connector("renamed-db", &["sql.*"], true),
            connector("hidden", &["browser.fetch"], false),
        ]);
        for (target, kind, expected) in [
            (
                "renamed-browser",
                "browser.fetch",
                Some(ProgressActivity::Browser),
            ),
            ("renamed-browser", "forkd.run", None),
            ("renamed-db", "sql.execute", Some(ProgressActivity::Sql)),
            (
                "renamed-db",
                "sql.future_operation",
                Some(ProgressActivity::Unknown),
            ),
            ("hidden", "browser.fetch", None),
            ("unknown", "browser.fetch", None),
        ] {
            assert_eq!(
                feed.tool_activity(
                    "dispatch_to_connector",
                    Some(&json!({"target": target, "kind": kind}))
                ),
                expected
            );
        }
        for arguments in [
            "invalid",
            "[]",
            r#"{"target":17,"kind":"browser.fetch"}"#,
            r#"{"target":"renamed-browser"}"#,
        ] {
            let item =
                feed.tool_progress_item("id".into(), "dispatch_to_connector", arguments, "running");
            assert_eq!(item.activity, None);
        }
    }

    #[test]
    fn projection_adds_only_a_closed_category_without_private_dispatch_payload() {
        let feed = StatusFeed::silent();
        feed.register_tool_names(["dispatch_to_connector".into()]);
        feed.register_connectors(&[connector("lookup", &["search.web"], true)]);
        let item = feed.tool_progress_item(
            "id".into(),
            "dispatch_to_connector",
            r#"{"target":"lookup","kind":"search.web","payload":{"token":"PRIVATE_PAYLOAD"}}"#,
            "running",
        );
        let wire = to_string(&item).unwrap();
        assert!(wire.contains("\"activity\":\"search\""));
        assert!(!wire.contains("PRIVATE_PAYLOAD"));
        assert!(!wire.contains("search.web"));
        assert_eq!(item.label, "Using lookup");
        for (kind, expected) in [
            ("storage.get", ProgressActivity::Files),
            ("forkd.run", ProgressActivity::Shell),
            ("calendar.create_event", ProgressActivity::Calendar),
            ("mail.cmd.read", ProgressActivity::Mail),
            ("imagegen.run", ProgressActivity::Image),
            ("transcribe.run", ProgressActivity::AudioInput),
            ("speak.run", ProgressActivity::AudioOutput),
        ] {
            assert_eq!(connector_activity(kind), expected);
        }
    }
}
