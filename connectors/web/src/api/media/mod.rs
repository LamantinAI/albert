use serde_json::{from_str, json};
use std::collections::HashMap;

use axum::{
    body::Body, http::HeaderMap, http::StatusCode, response::IntoResponse, response::Response, Json,
};

use super::{
    errors::ApiError, errors::ApiResult, models::AttachmentView, principal::Principal,
    router::resource_id, state::ApiState,
};

pub enum MediaOperation {
    Upload {
        bytes: Vec<u8>,
        mime: String,
        filename: String,
    },
    Delete {
        id: String,
    },
    Read {
        session: String,
        id: String,
        range: Option<String>,
        head: bool,
    },
}
pub enum MediaResponse {
    Uploaded(AttachmentView),
    Deleted,
    Asset {
        metadata: AttachmentView,
        bytes: Vec<u8>,
        range: Option<(usize, usize)>,
    },
    Unsatisfiable {
        size: usize,
    },
}
pub(crate) async fn request(
    state: &ApiState,
    caller: &Principal,
    method: &str,
    path: &str,
    params: &HashMap<String, String>,
    headers: &HeaderMap,
    bytes: &[u8],
) -> ApiResult<Option<Response>> {
    let header = |name| {
        headers
            .get(name)
            .and_then(|v| v.to_str().ok())
            .unwrap_or("")
            .to_owned()
    };
    let operation = match (method, path) {
        ("POST", "/attachments") => MediaOperation::Upload {
            bytes: bytes.to_vec(),
            mime: header("content-type"),
            filename: headers
                .get("x-filename")
                .and_then(|v| v.to_str().ok())
                .and_then(|v| from_str::<String>(v).ok())
                .unwrap_or_else(|| "attachment".into()),
        },
        ("DELETE", "/attachments/{id}") => MediaOperation::Delete {
            id: resource_id(params, "id")?,
        },
        ("GET" | "HEAD", "/sessions/{id}/attachments/{asset_id}") => MediaOperation::Read {
            session: resource_id(params, "id")?,
            id: resource_id(params, "asset_id")?,
            range: headers.get("range").map(|_| header("range")),
            head: method == "HEAD",
        },
        _ => return Ok(None),
    };
    let response = match state.host.media(caller, operation).await? {
        MediaResponse::Uploaded(asset) => Json(asset).into_response(),
        MediaResponse::Deleted => Json(json!({})).into_response(),
        MediaResponse::Unsatisfiable { size } => (
            StatusCode::RANGE_NOT_SATISFIABLE,
            [("content-range", format!("bytes */{size}"))],
        )
            .into_response(),
        MediaResponse::Asset {
            metadata,
            bytes,
            range,
        } => {
            let mut response = Response::new(Body::from(bytes));
            *response.status_mut() = if range.is_some() {
                StatusCode::PARTIAL_CONTENT
            } else {
                StatusCode::OK
            };
            let length = range
                .map(|(start, end)| end - start + 1)
                .unwrap_or(metadata.size);
            let headers = response.headers_mut();
            for (name, value) in [
                ("content-type", metadata.content_type.clone()),
                ("content-length", length.to_string()),
                ("accept-ranges", "bytes".into()),
                (
                    "content-security-policy",
                    "default-src 'none'; sandbox".into(),
                ),
            ] {
                headers.insert(name, value.parse().map_err(|_| ApiError::internal())?);
            }
            let mode = if metadata.content_type == "application/octet-stream" {
                "attachment"
            } else {
                "inline"
            };
            // JSON escaping is not HTTP header escaping: restrict the ASCII
            // fallback, and encode the Unicode filename separately in the UI.
            let name: String = metadata
                .filename
                .chars()
                .map(|c| {
                    if c.is_ascii_alphanumeric() || matches!(c, '.' | '-' | '_') {
                        c
                    } else {
                        '_'
                    }
                })
                .collect();
            let unicode_name = metadata
                .filename
                .bytes()
                .map(|b| {
                    if b.is_ascii_alphanumeric() || matches!(b, b'.' | b'-' | b'_' | b'~') {
                        (b as char).to_string()
                    } else {
                        format!("%{b:02X}")
                    }
                })
                .collect::<String>();
            headers.insert(
                "content-disposition",
                format!("{mode}; filename=\"{name}\"; filename*=UTF-8\'\'{unicode_name}")
                    .parse()
                    .map_err(|_| ApiError::internal())?,
            );
            if let Some((start, end)) = range {
                headers.insert(
                    "content-range",
                    format!("bytes {start}-{end}/{}", metadata.size)
                        .parse()
                        .map_err(|_| ApiError::internal())?,
                );
            }
            response
        }
    };
    Ok(Some(response))
}
