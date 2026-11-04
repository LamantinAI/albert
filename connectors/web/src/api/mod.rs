pub mod docs;
pub mod errors;
pub mod extractors;
pub mod host;
pub mod models;
pub mod principal;
mod router;
pub mod state;
#[cfg(test)]
mod testing;

#[cfg(test)]
pub use router::router;
pub use router::router_with_body_limit;

#[cfg(test)]
mod tests {
    use serde_json::{from_str, to_string_pretty, Value};

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
}
