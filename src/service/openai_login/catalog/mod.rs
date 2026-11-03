//! Minimal projection of the subscription catalog. Provider prompts and other
//! inference instructions never enter the control-plane response or model store.
use std::{collections::HashSet, time::Duration};

use octo_openai_auth::Subscription;
use reqwest::{redirect::Policy, Client};
use serde_json::{from_slice, Value};

use crate::api::models::SubscriptionCatalogModel;

const MAX_BODY: usize = 1024 * 1024;
/// Catalog protocol compatibility, independent of the assembly's release version.
const CLIENT_VERSION: &str = "0.162.1";

#[derive(Clone, Copy, Debug)]
pub(super) enum CatalogError {
    Unauthorized,
    Forbidden,
    RateLimited,
    Unavailable,
    Transport,
    TooLarge,
    Invalid,
}
impl CatalogError {
    pub fn message(self) -> &'static str {
        match self {
            Self::Unauthorized => "OpenAI did not accept the subscription credentials for its model catalog. Reconnect the subscription if retrying does not help.",
            Self::Forbidden => "OpenAI denied access to the subscription model catalog. Credentials remain connected. Review account access or the server network settings, then retry.",
            Self::RateLimited => "OpenAI rate-limited the model catalog request. Credentials remain connected. Wait briefly and retry.",
            Self::Unavailable => "OpenAI's model catalog is unavailable. Credentials remain connected. Retry later.",
            Self::Transport => "The server could not load OpenAI's model catalog. Credentials remain connected. Check server connectivity or proxy settings, then retry.",
            Self::TooLarge => "The model catalog exceeded its size limit. Credentials remain connected. Retry later.",
            Self::Invalid => "OpenAI returned an invalid model catalog. Credentials remain connected. Retry later.",
        }
    }
}
pub(super) async fn fetch(
    origin: &str,
    subscription: &Subscription,
) -> Result<Vec<SubscriptionCatalogModel>, CatalogError> {
    let client = Client::builder()
        .timeout(Duration::from_secs(20))
        .connect_timeout(Duration::from_secs(5))
        .redirect(Policy::none())
        .build()
        .map_err(|_| CatalogError::Transport)?;
    let mut response = client
        .get(format!("{}/models", origin.trim_end_matches('/')))
        .query(&[("client_version", CLIENT_VERSION)])
        .bearer_auth(&subscription.access_token)
        .header("ChatGPT-Account-ID", &subscription.account_id)
        .send()
        .await
        .map_err(|_| CatalogError::Transport)?;
    if !response.status().is_success() {
        return Err(match response.status().as_u16() {
            401 => CatalogError::Unauthorized,
            403 => CatalogError::Forbidden,
            429 => CatalogError::RateLimited,
            _ => CatalogError::Unavailable,
        });
    }
    if response
        .content_length()
        .is_some_and(|length| length > MAX_BODY as u64)
    {
        return Err(CatalogError::TooLarge);
    }
    let mut body = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .map_err(|_| CatalogError::Transport)?
    {
        if body.len() + chunk.len() > MAX_BODY {
            return Err(CatalogError::TooLarge);
        }
        body.extend_from_slice(&chunk);
    }
    project(&body)
}
fn text(value: Option<&Value>, maximum: usize) -> Option<String> {
    value
        .and_then(Value::as_str)
        .filter(|text| {
            !text.trim().is_empty()
                && text.len() <= maximum
                && !text
                    .chars()
                    .any(|c| c.is_control() && c != '\n' && c != '\t')
        })
        .map(str::to_owned)
}
fn project(body: &[u8]) -> Result<Vec<SubscriptionCatalogModel>, CatalogError> {
    let value: Value = from_slice(body).map_err(|_| CatalogError::Invalid)?;
    let models = value
        .get("models")
        .and_then(Value::as_array)
        .filter(|models| models.len() <= 512)
        .ok_or(CatalogError::Invalid)?;
    let mut projected = Vec::new();
    let mut seen = HashSet::new();
    for entry in models {
        if entry.get("hidden").and_then(Value::as_bool) == Some(true)
            || matches!(
                entry.get("visibility").and_then(Value::as_str),
                Some("hide" | "hidden" | "none")
            )
        {
            continue;
        }
        let slug = text(entry.get("slug"), 256)
            .filter(|slug| slug.bytes().all(|b| b.is_ascii_graphic()))
            .ok_or(CatalogError::Invalid)?;
        if !seen.insert(slug.clone()) {
            return Err(CatalogError::Invalid);
        }
        let name = text(entry.get("display_name"), 256).ok_or(CatalogError::Invalid)?;
        let input_modalities = match entry.get("input_modalities").and_then(Value::as_array) {
            Some(values) => values
                .iter()
                .filter_map(|value| text(Some(value), 64))
                .take(16)
                .collect(),
            // The catalog protocol defines the legacy modality default.
            None => vec!["text".into(), "image".into()],
        };
        let mut reasoning_efforts = Vec::new();
        for effort in entry
            .get("supported_reasoning_levels")
            .or_else(|| entry.get("supported_reasoning_efforts"))
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
            .take(16)
        {
            if let Some(effort) = text(
                effort
                    .get("effort")
                    .or_else(|| effort.get("reasoning_effort"))
                    .or(Some(effort)),
                64,
            ) {
                if !reasoning_efforts.contains(&effort) {
                    reasoning_efforts.push(effort);
                }
            }
        }
        let default_reasoning_effort = text(
            entry
                .get("default_reasoning_level")
                .or_else(|| entry.get("default_reasoning_effort")),
            64,
        )
        .filter(|effort| reasoning_efforts.contains(effort));
        projected.push(SubscriptionCatalogModel {
            slug,
            // This selects Codex's own tool runtime, not a Responses API
            // restriction. Do not disable advertised models based on it.
            selectable: true,
            unavailable_reason: None,
            name,
            description: text(entry.get("description"), 4096),
            input_modalities,
            context_window: entry
                .get("context_window")
                .and_then(Value::as_u64)
                .or_else(|| entry.get("max_context_window").and_then(Value::as_u64))
                .filter(|size| (1024..=10_000_000).contains(size))
                .map(|size| size as usize),
            reasoning_efforts,
            default_reasoning_effort,
        });
    }
    Ok(projected)
}

#[cfg(test)]
mod tests {
    use super::project;
    use serde_json::{json, to_string};

    #[test]
    fn projects_only_picker_metadata_and_maps_protocol_compatibility() {
        let projected = project(json!({"models":[
            {"slug":"chat","display_name":"Account Chat","description":"Provider description.","visibility":"list","supported_in_api":false,"input_modalities":["text","image"],"context_window":32000,"tool_mode":"direct","supported_reasoning_levels":[{"reasoning_effort":"high","description":"Longer thinking"}],"base_instructions":"private-prompt"},
            {"slug":"legacy","display_name":"Legacy","visibility":"list","max_context_window":64000},
            {"slug":"code-only","display_name":"Code only","visibility":"list","tool_mode":"code_mode_only"},
            {"slug":"hidden","display_name":"Hidden","visibility":"hide"},
            {"slug":"none","display_name":"None","visibility":"none"}
        ]}).to_string().as_bytes()).unwrap();
        assert_eq!(projected.len(), 3);
        assert_eq!(
            projected[0].description.as_deref(),
            Some("Provider description.")
        );
        assert_eq!(projected[0].reasoning_efforts, ["high"]);
        assert_eq!(projected[1].input_modalities, ["text", "image"]);
        assert_eq!(projected[1].context_window, Some(64000));
        assert!(projected[2].selectable);
        assert!(projected[2].unavailable_reason.is_none());
        let serialized = to_string(&projected).unwrap();
        assert!(!serialized.contains("private-prompt"));
        assert!(!serialized.contains("supported_in_api"));
    }
    #[test]
    fn invalid_catalogs_and_unknown_errors_do_not_echo_provider_text() {
        assert!(project(b"private-provider-body").is_err());
        assert!(project(br#"{"models":[{"slug":"x"}]}"#).is_err());
        assert!(project(br#"{"models":[]}"#).unwrap().is_empty());
    }

    #[test]
    fn codex_runtime_selectors_do_not_disable_subscription_models() {
        let models = ["direct", "code_mode", "code_mode_only", "future_mode"]
            .into_iter()
            .map(|mode| json!({"slug": mode, "display_name": mode, "visibility": "list", "tool_mode": mode}))
            .chain([json!({"slug":"legacy","display_name":"Legacy","visibility":"list"})])
            .collect::<Vec<_>>();
        let catalog = project(json!({"models":models}).to_string().as_bytes()).unwrap();
        assert_eq!(catalog.len(), 5);
        assert!(catalog
            .iter()
            .all(|model| model.selectable && model.unavailable_reason.is_none()));
    }
}
