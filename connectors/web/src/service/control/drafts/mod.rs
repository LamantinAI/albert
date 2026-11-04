//! Lossless owner-only transformations of an unsaved configuration draft.
mod comments;
mod fields;

use std::collections::HashSet;

use serde_json::{json, Value};
use toml_edit::{DocumentMut, InlineTable, Item, Table, Value as TomlValue};

use self::{comments::comments, comments::retain_removed_comments, fields::fields};
use super::{redact, ControlService};
use crate::{
    api::errors::ApiError, api::errors::ApiResult, api::models::ConfigDraftInput,
    api::models::ConfigDraftView, api::models::ConfigField, api::models::ConfigFieldKind,
};

fn scalar(value: &TomlValue, field: &ConfigField) -> ApiResult<Value> {
    let value = match field.kind {
        ConfigFieldKind::Boolean => value.as_bool().map(|v| json!(v)),
        ConfigFieldKind::Integer => value.as_integer().map(|v| json!(v)),
        ConfigFieldKind::Text => value.as_str().map(|v| json!(v)),
    }
    .ok_or_else(|| {
        ApiError::invalid(format!(
            "{} has an unsupported type. Edit it in Raw.",
            field.label
        ))
    })?;
    if value
        .as_i64()
        .is_some_and(|number| number.unsigned_abs() > 9_007_199_254_740_991)
    {
        return Err(ApiError::invalid(
            "This integer is too large for the form. Edit it in Raw.",
        ));
    }
    Ok(value)
}

fn validate(value: &Value, field: &ConfigField) -> ApiResult<()> {
    let valid = match field.kind {
        ConfigFieldKind::Boolean => value.is_boolean(),
        ConfigFieldKind::Text => value.is_string(),
        ConfigFieldKind::Integer => value.as_i64().is_some_and(|number| {
            field.minimum.is_none_or(|min| number >= min)
                && field.maximum.is_none_or(|max| number <= max)
        }),
    };
    if valid {
        Ok(())
    } else {
        Err(ApiError::invalid(format!(
            "Invalid value for {}.",
            field.label
        )))
    }
}

fn lookup<'a>(item: &'a Item, path: &[&str]) -> ApiResult<Option<&'a TomlValue>> {
    let Some((key, rest)) = path.split_first() else {
        return item.as_value().map(Some).ok_or_else(|| {
            ApiError::invalid("A supported setting is not a scalar. Edit it in Raw.")
        });
    };
    let table = item
        .as_table_like()
        .ok_or_else(|| ApiError::invalid("A setting group is not a table. Edit it in Raw."))?;
    match table.get(key) {
        Some(child) => lookup(child, rest),
        None => Ok(None),
    }
}

fn assign(item: &mut Item, path: &[&str], value: Option<TomlValue>) -> ApiResult<()> {
    let (key, rest) = path
        .split_first()
        .ok_or_else(|| ApiError::invalid("Invalid setting path."))?;
    let inline = item.is_inline_table();
    let table = item.as_table_like_mut().ok_or_else(|| {
        ApiError::invalid("A setting group conflicts with a scalar. Edit it in Raw.")
    })?;
    if rest.is_empty() {
        if let Some(mut value) = value {
            if let Some(old) = table.get_mut(key) {
                let decor = old
                    .as_value()
                    .ok_or_else(|| {
                        ApiError::invalid("A setting conflicts with a table. Edit it in Raw.")
                    })?
                    .decor()
                    .clone();
                *value.decor_mut() = decor;
                *old = Item::Value(value);
            } else {
                table.insert(key, Item::Value(value));
            }
        } else {
            table.remove(key);
        }
        return Ok(());
    }
    if !table.contains_key(key) {
        if value.is_none() {
            return Ok(());
        }
        let child = if inline {
            Item::Value(TomlValue::InlineTable(InlineTable::new()))
        } else {
            let mut table = Table::new();
            table.set_implicit(true);
            Item::Table(table)
        };
        table.insert(key, child);
    }
    assign(
        table
            .get_mut(key)
            .ok_or_else(|| ApiError::invalid("Cannot create this setting group."))?,
        rest,
        value,
    )
}

fn transform(input: ConfigDraftInput) -> ApiResult<ConfigDraftView> {
    if input.content.len() > 1024 * 1024 || input.changes.len() > 64 {
        return Err(ApiError::invalid(
            "Configuration draft exceeds its size limit.",
        ));
    }
    if redact(input.content.clone()).1 {
        return Err(ApiError::invalid(
            "This draft contains inline secrets. Use credential controls and Raw instead.",
        ));
    }
    let mut doc = input.content.parse::<DocumentMut>().map_err(|_| {
        ApiError::invalid("The draft is not valid TOML. Correct it in Raw before using the form.")
    })?;
    let mut fields = fields();
    let mut changed = false;
    let mut paths = HashSet::new();
    let mut removed_comments = Vec::new();
    for edit in &input.changes {
        if !paths.insert(&edit.path) {
            return Err(ApiError::invalid("Duplicate setting change."));
        }
        let field = fields
            .iter()
            .find(|field| field.path == edit.path)
            .ok_or_else(|| ApiError::invalid("This setting is available only in Raw."))?;
        let path: Vec<_> = field.path.split('.').collect();
        let old = lookup(doc.as_item(), &path)?
            .map(|value| scalar(value, field))
            .transpose()?;
        if edit.value.is_null() {
            if old.is_some() {
                removed_comments.extend(comments(doc.as_item(), &path));
                assign(doc.as_item_mut(), &path, None)?;
                changed = true;
            }
        } else {
            validate(&edit.value, field)?;
            if old.as_ref() == Some(&edit.value) {
                continue;
            }
            let value = match field.kind {
                ConfigFieldKind::Boolean => TomlValue::from(edit.value.as_bool().unwrap()),
                ConfigFieldKind::Integer => TomlValue::from(edit.value.as_i64().unwrap()),
                ConfigFieldKind::Text => TomlValue::from(edit.value.as_str().unwrap()),
            };
            assign(doc.as_item_mut(), &path, Some(value))?;
            changed = true;
        }
    }
    for field in &mut fields {
        field.value = lookup(doc.as_item(), &field.path.split('.').collect::<Vec<_>>())?
            .map(|value| scalar(value, field))
            .transpose()?;
    }
    let mut content = if changed {
        doc.to_string()
    } else {
        input.content.clone()
    };
    retain_removed_comments(&input.content, &mut content, removed_comments);
    if content.len() > 1024 * 1024 {
        return Err(ApiError::invalid("Configuration draft exceeds 1 MiB."));
    }
    Ok(ConfigDraftView {
        content,
        revision: input.revision,
        fields,
    })
}

impl ControlService {
    pub(super) async fn draft(
        &self,
        id: &str,
        input: ConfigDraftInput,
    ) -> ApiResult<ConfigDraftView> {
        if id != "albert" {
            return Err(ApiError::invalid("Structured fields are available for the agent configuration. Use the specialized editor or Raw for other documents."));
        }
        let _gate = self.gate.lock().await;
        let current = self.document(id)?;
        if current.revision != input.revision {
            return Err(ApiError::conflict(
                "Configuration changed. Reload before editing this draft.",
            ));
        }
        if current.redacted {
            return Err(ApiError::invalid("Inline secrets are redacted. Use credential controls before editing this document."));
        }
        transform(input)
    }
}

#[cfg(test)]
mod tests {
    use std::fs::write;

    use axum::http::StatusCode;
    use serde_json::{json, Value};
    use toml::{from_str, Table};

    use super::transform;
    use crate::{
        api::models::ConfigDraftInput, api::models::ConfigFieldChange,
        service::control::fixture::fixture,
    };

    fn input(content: &str, changes: &[(&str, Value)]) -> ConfigDraftInput {
        ConfigDraftInput {
            content: content.into(),
            revision: "revision".into(),
            changes: changes
                .iter()
                .map(|(path, value)| ConfigFieldChange {
                    path: (*path).into(),
                    value: value.clone(),
                })
                .collect(),
        }
    }
    #[test]
    fn changes_preserve_comments_quoted_keys_and_unknown_content() {
        let source = "# header\n\"timezone\" = 'UTC' # local time\nfuture = { key='keep exactly' }\n[\"agent\"] # section\n\"max_tool_turns\" = 8 # budget\n";
        let draft = transform(input(
            source,
            &[
                ("agent.max_tool_turns", json!(9)),
                ("timezone", json!("Europe/Paris")),
            ],
        ))
        .unwrap();
        for text in [
            "# header",
            "future = { key='keep exactly' }",
            "[\"agent\"] # section",
            "\"max_tool_turns\" = 9 # budget",
            "# local time",
        ] {
            assert!(draft.content.contains(text), "{text}");
        }
        let parsed: Table = from_str(&draft.content).unwrap();
        assert_eq!(parsed["timezone"].as_str(), Some("Europe/Paris"));
    }
    #[test]
    fn dotted_and_inline_tables_keep_unrelated_syntax() {
        for source in [
            "agent.max_tool_turns=8 # preserve\n",
            "agent = { max_tool_turns = 8, future='preserve' }\n",
        ] {
            let draft = transform(input(source, &[("agent.max_tool_turns", json!(11))])).unwrap();
            let parsed: Table = from_str(&draft.content).unwrap();
            assert_eq!(parsed["agent"]["max_tool_turns"].as_integer(), Some(11));
            assert!(draft.content.contains("preserve"));
            if source.contains("future") {
                assert!(draft.content.contains("future='preserve'"));
            }
        }
        let draft = transform(input(
            "sessions = { titles = { enabled = true }, future='keep' }\n",
            &[
                ("sessions.titles.enabled", json!(false)),
                ("sessions.limits.temporary_max_turns", json!(20)),
            ],
        ))
        .unwrap();
        let parsed: Table = from_str(&draft.content).unwrap();
        assert_eq!(
            parsed["sessions"]["titles"]["enabled"].as_bool(),
            Some(false)
        );
        assert_eq!(
            parsed["sessions"]["limits"]["temporary_max_turns"].as_integer(),
            Some(20)
        );
        assert!(draft.content.contains("future='keep'"));
    }
    #[test]
    fn no_op_projection_keeps_exact_bytes_and_does_not_insert_defaults() {
        let source = "# spacing\r\n\"timezone\"  =  'UTC' # keep\r\n";
        for changes in [
            vec![],
            vec![("timezone", json!("UTC"))],
            vec![("agent.max_tool_turns", Value::Null)],
        ] {
            let draft = transform(input(source, &changes)).unwrap();
            assert_eq!(draft.content, source);
            let field = draft
                .fields
                .iter()
                .find(|field| field.path == "agent.max_tool_turns")
                .unwrap();
            assert!(field.value.is_none());
            assert!(field.default_value.as_i64().unwrap() > 0);
        }
    }
    #[test]
    fn insert_and_remove_overrides_keep_other_sections() {
        let source = "# header\n[future]\nkey = 'unchanged'\n";
        let draft = transform(input(source, &[("agent.max_tool_turns", json!(11))])).unwrap();
        assert!(draft.content.contains("key = 'unchanged'"));
        let reset = transform(input(
            &draft.content,
            &[("agent.max_tool_turns", Value::Null)],
        ))
        .unwrap();
        assert!(reset.content.contains("key = 'unchanged'"));
        assert!(!reset.content.contains("max_tool_turns"));
    }
    #[test]
    fn invalid_types_paths_collisions_and_secrets_are_rejected() {
        for changes in [
            vec![("web.bind", json!("0.0.0.0:80"))],
            vec![("stream_status", json!(0))],
            vec![("agent.max_tool_turns", json!(1.5))],
            vec![("agent.max_tool_turns", json!(-1))],
            vec![("context.new_messages_percent", json!(100))],
            vec![("timezone", json!(["UTC"]))],
            vec![
                ("stream_status", json!(true)),
                ("stream_status", json!(false)),
            ],
        ] {
            assert!(transform(input("", &changes)).is_err());
        }
        for content in [
            "agent = 'collision'",
            "[agent.max_tool_turns]\nx = 1",
            "timezone = [1, 2]",
            "not valid TOML",
            "api_key = 'secret-value'",
        ] {
            assert!(transform(input(content, &[])).is_err());
        }
        assert!(transform(input(&"#".repeat(1024 * 1024 + 1), &[])).is_err());
    }
    #[tokio::test]
    async fn service_is_read_only_and_checks_revision_and_redaction() {
        let (_dir, service, _caller) = fixture().await;
        let original = service.document("albert").unwrap();
        let mut request = input("# draft only\n", &[("agent.max_tool_turns", json!(9))]);
        request.revision = original.revision.clone();
        let draft = service.draft("albert", request.clone()).await.unwrap();
        assert!(draft.content.contains("max_tool_turns = 9"));
        assert_eq!(
            service.document("albert").unwrap().content,
            original.content
        );
        assert_eq!(
            service.document("albert").unwrap().revision,
            original.revision
        );
        request.revision = "stale".into();
        assert_eq!(
            service
                .draft("albert", request.clone())
                .await
                .err()
                .unwrap()
                .status,
            StatusCode::CONFLICT
        );
        assert!(service.draft("models", request.clone()).await.is_err());
        write(&service.runtime.path, "api_key = 'redacted-value'").unwrap();
        request.revision = service.document("albert").unwrap().revision;
        assert!(service.draft("albert", request).await.is_err());
    }
    #[test]
    fn reset_retains_comments_and_dotted_inline_sources_stay_valid() {
        for source in [
            "# preserve explanation\nagent.max_tool_turns = 8 # budget note\n",
            "# preserve explanation\n[agent]\nmax_tool_turns = 8 # budget note\n",
            "agent = { max_tool_turns = 8, future='preserved' }\n",
        ] {
            let draft = transform(input(source, &[("agent.max_tool_turns", Value::Null)])).unwrap();
            let parsed: Table = from_str(&draft.content).unwrap();
            assert!(parsed
                .get("agent")
                .and_then(|v| v.get("max_tool_turns"))
                .is_none());
            for comment in ["# preserve explanation", "# budget note"] {
                if source.contains(comment) {
                    assert!(draft.content.contains(comment));
                }
            }
        }
        let source = "context.enabled = false # unchanged\n";
        let draft = transform(input(source, &[("context.reserve_tokens", json!(40000))])).unwrap();
        let parsed: Table = from_str(&draft.content).unwrap();
        assert_eq!(
            parsed["context"]["reserve_tokens"].as_integer(),
            Some(40000)
        );
        assert_eq!(parsed["context"]["enabled"].as_bool(), Some(false));
        let zeros = transform(input("[skills]\ncache = 0\npage = 0\n", &[])).unwrap();
        assert_eq!(
            zeros
                .fields
                .iter()
                .find(|f| f.path == "skills.cache")
                .unwrap()
                .value,
            Some(json!(0))
        );
    }
}
