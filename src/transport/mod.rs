//! Preserve typed transport failures before the provider SDK flattens SSE errors.
mod status;
mod stream;
pub use status::provider_status_code;
pub(crate) use stream::responses_stream;

use std::{
    error::Error as StdError,
    io::{Error as IoError, ErrorKind},
};

use mcp_http::Error as ReqwestError;
use rig::{completion::CompletionError, http_client::Error as HttpError};
use thiserror::Error;

pub const TIMEOUT: &str = "ALBERT_TRANSPORT_TIMEOUT";
pub const CONNECTION: &str = "ALBERT_TRANSPORT_CONNECTION";
pub const INCOMPLETE: &str = "ALBERT_TRANSPORT_INCOMPLETE";

#[derive(Debug, Error)]
enum Fault {
    #[error("ALBERT_TRANSPORT_TIMEOUT: model HTTP request timed out")]
    Timeout,
    #[error("ALBERT_TRANSPORT_CONNECTION: model HTTP connection interrupted")]
    Connection,
    #[error("ALBERT_TRANSPORT_INCOMPLETE: response stream ended before completion")]
    Incomplete,
}

fn incomplete() -> HttpError {
    HttpError::Instance(Box::new(Fault::Incomplete))
}

pub fn normalize(error: HttpError) -> HttpError {
    let mut source: Option<&(dyn StdError + 'static)> = Some(&error);
    while let Some(cause) = source {
        let fault = if let Some(error) = cause.downcast_ref::<ReqwestError>() {
            if error.is_timeout() {
                Some(Fault::Timeout)
            } else if error.is_connect() || error.is_body() {
                Some(Fault::Connection)
            } else {
                None
            }
        } else if let Some(error) = cause.downcast_ref::<IoError>() {
            match error.kind() {
                ErrorKind::TimedOut => Some(Fault::Timeout),
                ErrorKind::ConnectionReset
                | ErrorKind::ConnectionAborted
                | ErrorKind::BrokenPipe
                | ErrorKind::UnexpectedEof => Some(Fault::Connection),
                _ => None,
            }
        } else {
            None
        };
        if let Some(fault) = fault {
            return HttpError::Instance(Box::new(fault));
        }
        source = cause.source();
    }
    error
}

pub fn is_transient(message: &str) -> bool {
    [TIMEOUT, CONNECTION, INCOMPLETE]
        .iter()
        .any(|marker| message.contains(marker))
}

pub fn completion_is_transient(error: &CompletionError) -> bool {
    let text = error.to_string();
    if is_transient(&text) {
        return true;
    }
    if let Some(status) = provider_status_code(&text) {
        return matches!(status, 408 | 429 | 500 | 502 | 503 | 504);
    }
    let lower = text.to_ascii_lowercase();
    [
        "server_is_overloaded",
        "overloaded",
        "rate_limit",
        "temporarily unavailable",
        "timed out",
        "connection reset",
        "connection closed",
    ]
    .iter()
    .any(|hint| lower.contains(hint))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn typed_io_timeouts_and_disconnects_survive_sdk_stringification_without_details() {
        for kind in [
            ErrorKind::TimedOut,
            ErrorKind::ConnectionReset,
            ErrorKind::UnexpectedEof,
        ] {
            let error = normalize(HttpError::Instance(Box::new(IoError::new(
                kind,
                "secret-url-token",
            ))));
            assert!(is_transient(&error.to_string()));
            assert!(!error.to_string().contains("secret-url-token"));
        }
        let error = normalize(HttpError::Instance(Box::new(IoError::new(
            ErrorKind::InvalidData,
            "malformed payload",
        ))));
        assert!(!is_transient(&error.to_string()));
    }
    #[tokio::test]
    async fn terminal_detection_handles_chunked_crlf_and_does_not_match_text_content() {
        use bytes::Bytes;
        use futures::{
            stream::{iter, pending},
            StreamExt,
        };
        use rig::http_client::sse::BoxedStream;
        use std::{convert::Infallible, time::Duration};
        use tokio::time::timeout;
        let text = "data: {\"type\":\"response.output_text.delta\",\"delta\":\"response.completed Привет\"}\r\n\r\ndata: {\"type\":\"response.completed\"}\r\n\r\n";
        let chunks: Vec<_> = text
            .as_bytes()
            .chunks(3)
            .map(|c| Ok::<_, Infallible>(Bytes::copy_from_slice(c)))
            .collect();
        let body: BoxedStream = Box::pin(
            iter(chunks)
                .map(|c| c.map_err(|never| match never {}))
                .chain(pending()),
        );
        let received = timeout(
            Duration::from_secs(2),
            responses_stream(body).collect::<Vec<_>>(),
        )
        .await
        .unwrap();
        let bytes: Vec<_> = received
            .into_iter()
            .flat_map(|c| c.unwrap().to_vec())
            .collect();
        assert_eq!(bytes, text.as_bytes());
    }
}
