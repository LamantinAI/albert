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
    use super::docs::schema;
    use serde_json::to_string_pretty;
    use std::{fs::write, path::Path};

    #[test]
    #[ignore = "regenerates the checked-in public API contract"]
    fn regenerate_contract() {
        write(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("contracts/web-panel.openapi.json"),
            format!("{}\n", to_string_pretty(&schema()).unwrap()),
        )
        .unwrap();
    }
}
