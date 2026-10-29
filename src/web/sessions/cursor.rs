use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{from_slice, to_vec};

use crate::api::errors::{ApiError, ApiResult};

#[derive(Serialize, Deserialize)]
#[serde(tag = "kind", deny_unknown_fields)]
pub(super) enum Cursor {
    Messages {
        owner: String,
        session: String,
        before: Option<i64>,
        through: i64,
    },
    Catalog {
        owner: String,
        temporary: CatalogPosition,
        durable: CatalogPosition,
    },
}
#[derive(Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct CatalogPosition {
    pub before: Option<i64>,
    pub through: Option<i64>,
    pub done: bool,
}
impl Cursor {
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(to_vec(self).expect("cursor serializes"))
    }
    pub fn decode(value: &str) -> ApiResult<Self> {
        if value.len() > 4096 {
            return Err(ApiError::invalid("Invalid page cursor."));
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(value)
            .map_err(|_| ApiError::invalid("Invalid page cursor."))?;
        from_slice(&bytes).map_err(|_| ApiError::invalid("Invalid page cursor."))
    }
}
