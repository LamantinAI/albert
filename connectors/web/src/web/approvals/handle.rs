use base64::{engine::general_purpose::URL_SAFE_NO_PAD, Engine};
use serde::{Deserialize, Serialize};
use serde_json::{from_slice, to_vec};

use crate::api::{errors::ApiError, errors::ApiResult};

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum ApprovalHandle {
    Confirmation {
        request: String,
    },
    Native {
        adapter: String,
        executor: String,
        operation: String,
    },
}
impl ApprovalHandle {
    pub fn encode(&self) -> String {
        URL_SAFE_NO_PAD.encode(to_vec(self).expect("identifiers serialize"))
    }
    pub fn decode(id: &str) -> ApiResult<Self> {
        if id.len() > 4096 {
            return Err(ApiError::not_found());
        }
        let bytes = URL_SAFE_NO_PAD
            .decode(id)
            .map_err(|_| ApiError::not_found())?;
        from_slice(&bytes)
            .or_else(|_| {
                from_slice::<(String, String)>(&bytes).map(|(executor, operation)| Self::Native {
                    adapter: "sql".into(),
                    executor,
                    operation,
                })
            })
            .map_err(|_| ApiError::not_found())
    }
}
