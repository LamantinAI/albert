//! Discovery over the same explicitly advertised surface as connector dispatch.
//! Never expose unadvertised control kinds or raw manifests/credentials.
use std::{convert::Infallible, sync::Arc};

use octo_core::ConnectorInfo;
use rig::{completion::ToolDefinition, tool::Tool};
use serde::Deserialize;
use serde_json::{json, Value};

#[derive(Clone)]
pub struct ConnectorCatalog(Arc<Vec<Entry>>);

struct Entry {
    target: String,
    description: String,
    hints: String,
}

/// Task vocabulary for the primary connector protocols. IDs remain runtime data.
const ROUTES: &[(&str, &str)] = &[
    (
        "search.web",
        "Web search / поиск в интернете: find sources and current facts",
    ),
    (
        "browser.fetch",
        "Read URL / браузер, прочитать страницу: extract a known web page",
    ),
    (
        "commons.cmd.search",
        "Reference photos / фотографии, иллюстрации: search Wikimedia Commons",
    ),
    (
        "transcribe.run",
        "Hear audio / расшифровка, транскрипция: audio or video to text",
    ),
    ("speak.run", "Speak / голос: synthesize a voice reply"),
    (
        "imagegen.run",
        "Generate images / нарисовать: create or edit artwork",
    ),
    (
        "forkd.run",
        "Scripts / скрипты: local computation or a missing integration, after connector discovery",
    ),
];

impl ConnectorCatalog {
    pub fn new(connectors: &[ConnectorInfo]) -> Self {
        let mut entries: Vec<_> = connectors
            .iter()
            .filter_map(|c| {
                let description = c.capabilities.description.as_ref()?.trim();
                if description.is_empty() {
                    return None;
                }
                let hints = ROUTES
                    .iter()
                    .filter(|(kind, _)| {
                        c.capabilities
                            .event_kinds_accept
                            .iter()
                            .any(|k| k.as_str() == *kind)
                            && description.contains(kind)
                    })
                    .map(|(_, hint)| *hint)
                    .collect::<Vec<_>>()
                    .join("; ");
                Some(Entry {
                    target: c.id.to_string(),
                    description: description.into(),
                    hints,
                })
            })
            .collect();
        entries.sort_by(|a, b| a.target.cmp(&b.target));
        Self(Arc::new(entries))
    }

    fn search(&self, query: &str, limit: usize) -> Value {
        let query = query.trim().to_lowercase();
        if query.is_empty() {
            return json!({"targets":self.0.iter().map(|e| json!({"target":e.target,"summary":if e.hints.is_empty() {e.description.lines().next().unwrap_or("")} else {&e.hints}})).collect::<Vec<_>>(),"hint":"Search by target ID to retrieve its complete advertised command contract."});
        }
        let terms: Vec<_> = query
            .split(|c: char| !(c.is_alphanumeric() || matches!(c, '_' | '.' | '-')))
            .filter(|s| !s.is_empty())
            .collect();
        let mut matches: Vec<_> = self
            .0
            .iter()
            .filter_map(|e| {
                let id = e.target.to_lowercase();
                let haystack = format!("{id}\n{}\n{}", e.description, e.hints).to_lowercase();
                let score = if id == query {
                    10_000
                } else {
                    terms.iter().filter(|t| haystack.contains(**t)).count()
                };
                (score > 0).then_some((score, e))
            })
            .collect();
        matches.sort_by(|(a, ea), (b, eb)| b.cmp(a).then(ea.target.cmp(&eb.target)));
        let total = matches.len();
        let results: Vec<_> = matches
            .into_iter()
            .take(limit.clamp(1, 10))
            .map(|(_, e)| json!({"target":e.target,"description":e.description}))
            .collect();
        json!({"matches":results,"total":total,"available_targets":self.0.iter().map(|e| &e.target).collect::<Vec<_>>(),"hint":"Use the exact target, command kind and payload from the advertised description with dispatch_to_connector. If the task is not covered, inspect skills before writing an integration script. Discovery does not grant additional permissions."})
    }
}

#[derive(Deserialize)]
pub struct SearchArgs {
    #[serde(default)]
    pub query: String,
    #[serde(default = "default_limit")]
    pub limit: usize,
}
fn default_limit() -> usize {
    5
}

impl Tool for ConnectorCatalog {
    const NAME: &'static str = "connector_search";
    type Error = Infallible;
    type Args = SearchArgs;
    type Output = Value;

    async fn definition(&self, _prompt: String) -> ToolDefinition {
        ToolDefinition { name:Self::NAME.into(), description:"Find an existing connector for a task BEFORE writing network/integration scripts or claiming a capability is missing. Search by task keywords, command kind, or exact target ID; English keywords work best for unfamiliar connectors. An empty query lists every advertised connector; an exact target query returns its full public command description. Use this for less familiar capabilities, too. This is discovery only, not execution or an access grant.".into(), parameters:json!({"type":"object","properties":{"query":{"type":"string","description":"Task keywords, command name, or target ID; empty lists all"},"limit":{"type":"integer","minimum":1,"maximum":10}},"additionalProperties":false}) }
    }
    async fn call(&self, args: SearchArgs) -> Result<Value, Infallible> {
        Ok(self.search(&args.query, args.limit))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use octo_core::{ConnectorCapabilities, ConnectorId, EventKind};

    fn entry(id: &str, text: &str, kinds: &[&str]) -> ConnectorInfo {
        ConnectorInfo {
            id: ConnectorId::new(id),
            capabilities: ConnectorCapabilities::bidirectional()
                .with_description(text)
                .with_accept_kinds(kinds.iter().map(|k| EventKind::new(*k))),
        }
    }

    #[test]
    fn renamed_primary_and_rare_connectors_are_visible_and_searchable() {
        let mut entries = (0..18)
            .map(|i| entry(&format!("misc-{i}"), "Some connector", &[]))
            .collect::<Vec<_>>();
        entries.push(entry(
            "internet-work",
            "search.web { query } searches the web",
            &["search.web"],
        ));
        entries.push(entry(
            "warehouse",
            "inventory.reserve { sku, quantity } reserves stock",
            &["inventory.reserve"],
        ));
        let catalog = ConnectorCatalog::new(&entries);
        assert!(catalog.search("", 1).to_string().contains("Web search"));
        assert!(catalog.search("", 1).to_string().contains("warehouse"));
        assert_eq!(
            catalog.search("интернет", 5)["matches"][0]["target"],
            "internet-work"
        );
        assert_eq!(
            catalog.search("inventory reserve", 5)["matches"][0]["target"],
            "warehouse"
        );
        assert_eq!(
            catalog.search("", 1)["targets"].as_array().unwrap().len(),
            20
        );
    }

    #[test]
    fn discovery_exposes_only_the_advertised_contract_and_no_invented_capabilities() {
        let catalog = ConnectorCatalog::new(&[entry(
            "channel",
            "chat.send_file { path }",
            &["chat.send_file", "private.admin"],
        )]);
        let result = catalog.search("channel", 5).to_string();
        assert!(!result.contains("private.admin"));
        assert!(!catalog.search("", 1).to_string().contains("search.web"));
        assert_eq!(catalog.search("unavailable", 5)["total"], 0);
        assert_eq!(
            catalog.search("channel", 0)["matches"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
    }
}
