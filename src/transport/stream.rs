use futures::{stream::unfold, StreamExt};
use rig::http_client::sse::BoxedStream;
use serde_json::{from_slice, Value};

use super::{incomplete, normalize};

/// Preserve bytes and errors, but require a terminal Responses event. Stop after
/// that event rather than waiting indefinitely for the server to close the socket.
pub(crate) fn responses_stream(body: BoxedStream) -> BoxedStream {
    Box::pin(unfold(
        (body, Terminal::default(), false),
        |(mut body, mut monitor, finished)| async move {
            if finished {
                return None;
            }
            match body.next().await {
                Some(Ok(bytes)) => {
                    monitor.observe(&bytes);
                    let finished = monitor.complete;
                    Some((Ok(bytes), (body, monitor, finished)))
                }
                Some(Err(error)) => Some((Err(normalize(error)), (body, monitor, true))),
                None => Some((Err(incomplete()), (body, monitor, true))),
            }
        },
    ))
}

#[derive(Default)]
struct Terminal {
    line: Vec<u8>,
    data: Vec<u8>,
    complete: bool,
}
impl Terminal {
    fn observe(&mut self, bytes: &[u8]) {
        for piece in bytes.split_inclusive(|b| *b == b'\n') {
            self.line.extend_from_slice(piece);
            if piece.last() != Some(&b'\n') {
                continue;
            }
            let line = self.line.strip_suffix(b"\n").unwrap_or(&self.line);
            let line = line.strip_suffix(b"\r").unwrap_or(line);
            if line.is_empty() {
                if let Ok(event) = from_slice::<Value>(&self.data) {
                    self.complete |= matches!(
                        event["type"].as_str(),
                        Some("response.completed" | "response.failed" | "response.incomplete")
                    );
                }
                self.data.clear();
            } else if let Some(data) = line.strip_prefix(b"data:") {
                self.data
                    .extend_from_slice(data.strip_prefix(b" ").unwrap_or(data));
                self.data.push(b'\n');
            }
            self.line.clear();
        }
    }
}
