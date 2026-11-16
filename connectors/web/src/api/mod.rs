pub mod docs;
pub mod errors;
pub mod extractors;
pub mod host;
pub mod mcp;
pub mod media;
pub mod memory;
pub mod models;
pub mod principal;
mod router;
pub mod state;
#[cfg(test)]
pub(crate) mod testing;

#[cfg(test)]
pub use router::router;
pub use router::{router_with_body_limit, router_with_media_limit};

#[cfg(test)]
mod tests {
    use serde_json::{from_str, to_string_pretty, Value};
    use std::collections::HashSet;

    use super::docs::schema;

    #[test]
    fn regenerate_contract() {
        let document = schema();
        let exported = format!("{}\n", to_string_pretty(&document).unwrap());
        assert_eq!(from_str::<Value>(&exported).unwrap(), document);
        assert_eq!(
            exported,
            format!("{}\n", to_string_pretty(&schema()).unwrap())
        );
        assert_eq!(document["openapi"], "3.1.0");
        assert!(document["paths"]["/sessions"].is_object());
    }

    #[test]
    fn operation_ids_are_unique_and_domain_tags_are_present() {
        let document = schema();
        let mut ids = HashSet::new();
        for (path, item) in document["paths"].as_object().unwrap() {
            for method in ["get", "post", "put", "patch", "delete", "head"] {
                let Some(operation) = item.get(method) else {
                    continue;
                };
                let id = operation["operationId"].as_str().unwrap();
                assert!(ids.insert(id), "Duplicate operation ID: {id}");
                assert!(
                    !operation["tags"].as_array().unwrap().is_empty(),
                    "Missing domain: {method} {path}"
                );
            }
        }
    }

    #[test]
    fn binary_and_event_contracts_describe_their_actual_wire_content() {
        let document = schema();
        assert_eq!(
            document["components"]["schemas"]["BinaryContent"]["type"],
            "string"
        );
        assert_eq!(
            document["components"]["schemas"]["BinaryContent"]["format"],
            "binary"
        );
        let file = &document["paths"]["/sessions/{id}/attachments/{asset_id}"];
        assert!(file["get"]["responses"]["206"]["content"]["application/octet-stream"].is_object());
        assert!(file["head"]["responses"]["200"].get("content").is_none());
        assert!(
            document["paths"]["/sessions/{id}/events"]["get"]["responses"]["200"]["content"]
                ["text/event-stream"]
                .is_object()
        );
    }
}
