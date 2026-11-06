use albert_core::{
    media::Asset, media::MediaError, media::MediaInput, service::session::web::WebActor,
};
use octo_history::SessionKind;

use super::SessionHost;
use crate::api::{
    errors::ApiError, errors::ApiResult, media::MediaOperation, media::MediaResponse,
    models::AttachmentView, principal::Principal,
};

pub(super) fn asset_view(asset: Asset) -> AttachmentView {
    AttachmentView {
        id: asset.id,
        filename: asset.filename,
        content_type: asset.content_type,
        size: asset.size,
    }
}
fn error(error: MediaError) -> ApiError {
    match error {
        MediaError::NotFound | MediaError::Scope => ApiError::not_found(),
        MediaError::Limit => {
            ApiError::invalid("Attachment limits exceeded. Remove a file or try a smaller one.")
        }
        MediaError::Type => {
            ApiError::invalid("The file type does not match its contents, or is unsupported.")
        }
        _ => ApiError::unavailable("The attachment could not be read or stored."),
    }
}
pub(super) fn valid_message(text: &str, ids: &[String]) -> ApiResult<()> {
    if (text.trim().is_empty() && ids.is_empty()) || text.len() > 1024 * 1024 {
        return Err(ApiError::invalid(
            "Send text or an attachment. Message text must not exceed 1048576 bytes.",
        ));
    }
    Ok(())
}
impl SessionHost {
    pub(super) async fn prepare_media(
        &self,
        actor: &WebActor,
        session: Option<&str>,
        text: &str,
        ids: &[String],
    ) -> ApiResult<MediaInput> {
        let _lease = if let Some(id) = session {
            let context = self.resource(actor, id).await?;
            Some(
                self.runtime
                    .sessions
                    .journal_router()
                    .handle(context.session_id())
                    .await
                    .read_owned()
                    .await,
            )
        } else {
            None
        };
        let prepared = self
            .runtime
            .sessions
            .media()
            .prepare(&actor.account_id, session, ids)
            .await
            .map_err(error)?;
        Ok(MediaInput {
            text: text.into(),
            assets: prepared.assets(),
            blobs: prepared.blobs(),
        })
    }
    pub async fn media(
        &self,
        caller: &Principal,
        operation: MediaOperation,
    ) -> ApiResult<MediaResponse> {
        let actor = self.actor(caller).await?;
        let store = self.runtime.sessions.media();
        match operation {
            MediaOperation::Upload {
                bytes,
                mime,
                filename,
            } => {
                let asset = store
                    .stage(&actor.account_id, bytes, &mime, &filename)
                    .await
                    .map_err(error)?;
                Ok(MediaResponse::Uploaded(asset_view(asset)))
            }
            MediaOperation::Delete { id } => {
                store
                    .remove_staged(&actor.account_id, &id)
                    .await
                    .map_err(error)?;
                Ok(MediaResponse::Deleted)
            }
            MediaOperation::Read {
                session,
                id,
                range,
                head,
            } => {
                let context = self.resource(&actor, &session).await?;
                let lease = self
                    .runtime
                    .sessions
                    .journal_router()
                    .handle(context.session_id())
                    .await
                    .read_owned()
                    .await;
                let metadata = self
                    .runtime
                    .sessions
                    .get(&context, context.session_id())
                    .await
                    .map_err(super::session_error)?;
                if !context.is_live() {
                    return Err(ApiError::not_found());
                }
                let temporary = metadata.kind == SessionKind::Temporary;
                let asset = store
                    .metadata(&session, &id, temporary)
                    .await
                    .map_err(error)?;
                let selected = match range
                    .as_deref()
                    .map(|r| byte_range(r, asset.size))
                    .transpose()
                {
                    Ok(range) => range,
                    Err(()) => return Ok(MediaResponse::Unsatisfiable { size: asset.size }),
                };
                let (start, end) = selected.unwrap_or((0, asset.size - 1));
                let bytes = if head {
                    vec![]
                } else {
                    store
                        .slice(&session, &id, temporary, start, end - start + 1)
                        .await
                        .map_err(error)?
                };
                // Recheck live credentials after I/O, before returning private bytes.
                self.actor(caller).await?;
                if !context.is_live() {
                    return Err(ApiError::not_found());
                }
                drop(lease);
                Ok(MediaResponse::Asset {
                    metadata: asset_view(asset),
                    bytes,
                    range: selected,
                })
            }
        }
    }
}
fn byte_range(value: &str, size: usize) -> Result<(usize, usize), ()> {
    let value = value.strip_prefix("bytes=").ok_or(())?;
    if value.contains(',') || size == 0 {
        return Err(());
    }
    let (start, end) = value.split_once('-').ok_or(())?;
    if start.is_empty() {
        if !end.bytes().all(|b| b.is_ascii_digit()) {
            return Err(());
        }
        let suffix = end.parse::<usize>().map_err(|_| ())?;
        if suffix == 0 {
            return Err(());
        }
        return Ok((size.saturating_sub(suffix), size - 1));
    }
    if !start.bytes().all(|b| b.is_ascii_digit()) || !end.bytes().all(|b| b.is_ascii_digit()) {
        return Err(());
    }
    let start = start.parse::<usize>().map_err(|_| ())?;
    let end = if end.is_empty() {
        size - 1
    } else {
        end.parse::<usize>().map_err(|_| ())?.min(size - 1)
    };
    if start >= size || end < start {
        return Err(());
    }
    Ok((start, end))
}

#[cfg(test)]
mod tests {
    use super::byte_range;
    #[test]
    fn ranges_cover_seek_suffix_and_invalid_requests() {
        assert_eq!(byte_range("bytes=3-7", 10), Ok((3, 7)));
        assert_eq!(byte_range("bytes=3-", 10), Ok((3, 9)));
        assert_eq!(byte_range("bytes=-3", 10), Ok((7, 9)));
        assert_eq!(byte_range("bytes=8-90", 10), Ok((8, 9)));
        for value in [
            "bytes=10-",
            "bytes=-0",
            "bytes=8-3",
            "bytes=0-1,3-4",
            "bytes=+1-4",
            "invalid",
        ] {
            assert!(byte_range(value, 10).is_err(), "{value}");
        }
    }
}

#[cfg(test)]
mod boundary_tests {
    use albert_core::media::encode;
    use axum::{body::to_bytes, body::Body, http::Request, http::StatusCode};
    use octo_history::{AppendRequest, HistoryStore, Turn};
    use serde_json::{from_slice, Value};
    use tower::ServiceExt;

    use super::super::fixture::fixture;
    use crate::{
        api::models::LoginRequest, api::router_with_media_limit, api::state::ApiState, web::Panel,
    };

    #[tokio::test]
    async fn binary_upload_range_head_revocation_and_temporary_end_use_real_auth() {
        let (_dir, host, caller) = fixture().await;
        let login = host
            .auth
            .login(LoginRequest {
                username: "owner".into(),
                password: "web-host-test-password".into(),
            })
            .await
            .unwrap();
        let principal = host.auth.authenticate(&login.token).await.unwrap();
        let cookie = format!("albert_auth={}", login.token);
        let router = router_with_media_limit(
            ApiState::new(
                host.auth.clone(),
                Panel::new(host.runtime.clone(), host.auth.clone()),
            ),
            1024,
            4096,
        );
        let bytes = b"\x89PNG\r\n\x1a\nsynthetic-original";
        let request = Request::post("/api/v1/attachments")
            .header("origin", "http://localhost:9877")
            .header("host", "localhost:9877")
            .header("cookie", &cookie)
            .header("x-albert-csrf", principal.csrf())
            .header("content-type", "image/png")
            .header("x-filename", "\"image.png\"")
            .body(Body::from(bytes.as_slice()))
            .unwrap();
        let response = router.clone().oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let uploaded: Value =
            from_slice(&to_bytes(response.into_body(), 4096).await.unwrap()).unwrap();
        let id = uploaded["id"].as_str().unwrap().to_owned();
        let actor = host.actor(&caller).await.unwrap();
        let session = host
            .runtime
            .sessions
            .web_create(&actor, "Media", true)
            .await
            .unwrap();
        let context = host.resource(&actor, session.id.as_str()).await.unwrap();
        let prepared = host
            .runtime
            .sessions
            .media()
            .prepare(&actor.account_id, None, &[id.clone()])
            .await
            .unwrap();
        host.runtime
            .sessions
            .journal_router()
            .append_journal(
                &context.root_history_key(),
                &AppendRequest {
                    namespace: "http-media".into(),
                    idempotency_key: "image".into(),
                    digest: "same".into(),
                    turns: vec![Turn::user(encode(String::new(), prepared.assets()))],
                },
            )
            .await
            .unwrap();
        let path = format!("/api/v1/sessions/{}/attachments/{id}", session.id);
        for (method, range, expected, length) in [
            ("GET", None, StatusCode::OK, bytes.len()),
            ("HEAD", None, StatusCode::OK, 0),
            ("GET", Some("bytes=2-6"), StatusCode::PARTIAL_CONTENT, 5),
            ("GET", Some("bytes=-4"), StatusCode::PARTIAL_CONTENT, 4),
            (
                "GET",
                Some("bytes=999-"),
                StatusCode::RANGE_NOT_SATISFIABLE,
                0,
            ),
        ] {
            let mut request = Request::builder()
                .method(method)
                .uri(&path)
                .header("cookie", &cookie);
            if let Some(range) = range {
                request = request.header("range", range);
            }
            let response = router
                .clone()
                .oneshot(request.body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), expected, "{method} {range:?}");
            assert_eq!(response.headers()["x-content-type-options"], "nosniff");
            let body = to_bytes(response.into_body(), 4096).await.unwrap();
            assert_eq!(body.len(), length);
            if range == Some("bytes=2-6") {
                assert_eq!(body.as_ref(), &bytes[2..7]);
            }
            if method == "GET" && range.is_none() {
                assert_eq!(body.as_ref(), bytes);
            }
        }
        let other = host
            .runtime
            .sessions
            .web_create(&actor, "Other", false)
            .await
            .unwrap();
        let wrong = format!("/api/v1/sessions/{}/attachments/{id}", other.id);
        let response = router
            .clone()
            .oneshot(
                Request::get(wrong)
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
        host.runtime
            .sessions
            .web_end(&actor, &session.id, session.revision)
            .await
            .unwrap();
        let response = router
            .clone()
            .oneshot(
                Request::get(&path)
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::GONE);
        host.auth
            .revoke_credential(&principal, principal.credential_id())
            .await
            .unwrap();
        let response = router
            .oneshot(
                Request::get(path)
                    .header("cookie", &cookie)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
    }
}
