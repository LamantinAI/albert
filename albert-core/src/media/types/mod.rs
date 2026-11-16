use serde::{Deserialize, Serialize};
use sqlx::Error as SqlError;
use thiserror::Error;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
pub struct Asset {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct MediaLimits {
    pub max_file_bytes: usize,
    pub max_attachments: usize,
    pub memory_bytes: usize,
    pub memory_files: usize,
    pub staged_account_files: usize,
    pub durable_files: i64,
    pub staged_account_bytes: usize,
    pub staged_ttl_seconds: u64,
    pub durable_bytes: i64,
}
impl Default for MediaLimits {
    fn default() -> Self {
        Self {
            max_file_bytes: 32 * 1024 * 1024,
            max_attachments: 8,
            memory_bytes: 256 * 1024 * 1024,
            memory_files: 256,
            staged_account_files: 64,
            durable_files: 100_000,
            staged_account_bytes: 64 * 1024 * 1024,
            staged_ttl_seconds: 3600,
            durable_bytes: 4 * 1024 * 1024 * 1024,
        }
    }
}
impl MediaLimits {
    pub fn valid(&self) -> bool {
        (1..=64 * 1024 * 1024).contains(&self.max_file_bytes)
            && (1..=16).contains(&self.max_attachments)
            && (self.max_attachments..=16384).contains(&self.memory_files)
            && (self.max_attachments..=self.memory_files).contains(&self.staged_account_files)
            && self.durable_files >= self.max_attachments as i64
            && self.memory_bytes >= self.max_file_bytes
            && self.staged_account_bytes >= self.max_file_bytes
            && (60..=86400).contains(&self.staged_ttl_seconds)
            && self.durable_bytes >= self.max_file_bytes as i64
    }
}

#[derive(Debug, Error)]
pub enum MediaError {
    #[error("Attachment not found or no longer available.")]
    NotFound,
    #[error("Attachment limits exceeded. Remove a file or try a smaller one.")]
    Limit,
    #[error("The file type does not match its contents, or is unsupported.")]
    Type,
    #[error("This attachment belongs to another conversation.")]
    Scope,
    #[error("The workspace file is unavailable.")]
    Workspace,
    #[error(transparent)]
    Database(#[from] SqlError),
}
pub type MediaResult<T> = Result<T, MediaError>;

/// Recognize container signatures rather than trusting browser MIME declarations.
/// Active document formats are never served inline.
pub(super) fn detect(bytes: &[u8]) -> &'static str {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        "image/png"
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        "image/jpeg"
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        "image/gif"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WEBP") {
        "image/webp"
    } else if bytes.starts_with(b"RIFF") && bytes.get(8..12) == Some(b"WAVE") {
        "audio/wav"
    } else if bytes.starts_with(b"OggS") {
        "audio/ogg"
    } else if bytes.starts_with(b"fLaC") {
        "audio/flac"
    } else if bytes.starts_with(b"ID3")
        || bytes
            .get(..2)
            .is_some_and(|b| b[0] == 0xff && b[1] & 0xe0 == 0xe0)
    {
        "audio/mpeg"
    } else if bytes.get(4..8) == Some(b"ftyp") {
        "video/mp4"
    } else if bytes.starts_with(b"\x1aE\xdf\xa3") {
        "video/webm"
    } else {
        "application/octet-stream"
    }
}
pub(super) fn content_type(bytes: &[u8], declared: &str) -> MediaResult<String> {
    let detected = detect(bytes);
    let declared = declared.split(';').next().unwrap_or("").trim();
    let native = matches!(
        declared,
        "image/png"
            | "image/jpeg"
            | "image/gif"
            | "image/webp"
            | "audio/wav"
            | "audio/x-wav"
            | "audio/ogg"
            | "audio/flac"
            | "audio/mpeg"
            | "audio/mp4"
            | "audio/x-m4a"
            | "audio/webm"
            | "video/mp4"
            | "video/webm"
    );
    // Unsupported media can share container signatures with native formats
    // (HEIC/MOV with MP4 or AAC with MPEG). Their declared type only permits
    // download, never native perception or inline rendering.
    if !native && !declared.is_empty() && declared != "application/octet-stream" {
        return Ok("application/octet-stream".into());
    }
    if detected == "application/octet-stream" {
        return if native {
            Err(MediaError::Type)
        } else {
            Ok(detected.into())
        };
    }
    let compatible = declared == detected
        || declared.is_empty()
        || declared == "application/octet-stream"
        || (detected == "audio/wav" && declared == "audio/x-wav")
        || (detected == "video/webm" && declared == "audio/webm")
        || (detected == "video/mp4" && matches!(declared, "audio/mp4" | "audio/x-m4a"));
    if !compatible {
        return Err(MediaError::Type);
    }
    Ok(if compatible
        && declared.starts_with("audio/")
        && matches!(detected, "video/webm" | "video/mp4")
    {
        declared
    } else {
        detected
    }
    .into())
}
pub(super) fn filename(value: &str) -> String {
    let name: String = value
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or("attachment")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    if name.trim().is_empty() {
        "attachment".into()
    } else {
        name
    }
}

#[cfg(test)]
mod tests {
    use super::{content_type, MediaError};

    #[test]
    fn documents_and_unsupported_media_are_download_only_without_trusting_mime() {
        for (bytes, mime) in [
            (b"name,value\na,1".as_slice(), "text/csv"),
            (b"%PDF-1.7\nfixture".as_slice(), "application/pdf"),
            (b"PK\x03\x04archive".as_slice(), "application/zip"),
            (b"<svg onload='run()'/>".as_slice(), "image/svg+xml"),
            (b"arbitrary\0binary".as_slice(), "vendor/custom"),
            (
                b"<script>run()</script>".as_slice(),
                "text/html; charset=utf-8",
            ),
            (b"\0\x01data".as_slice(), "audio/aac"),
            (b"\xff\xf1\x50\x80\x00\x1f\xfc".as_slice(), "audio/aac"),
            (b"\x00\x00\x00\x18ftypheicfixture".as_slice(), "image/heic"),
            (b"\x00\x00\x00\x18ftypaviffixture".as_slice(), "image/avif"),
            (
                b"\x00\x00\x00\x18ftypqt  fixture".as_slice(),
                "video/quicktime",
            ),
        ] {
            assert_eq!(
                content_type(bytes, mime).unwrap(),
                "application/octet-stream"
            );
        }
        for mime in [
            "image/png",
            "image/jpeg",
            "audio/ogg",
            "audio/webm",
            "video/mp4",
        ] {
            assert!(matches!(
                content_type(b"not native media", mime),
                Err(MediaError::Type)
            ));
        }
        assert_eq!(
            content_type(b"\x89PNG\r\n\x1a\nimage", "image/png").unwrap(),
            "image/png"
        );
        assert_eq!(
            content_type(b"RIFF\0\0\0\0WAVEaudio", "audio/x-wav").unwrap(),
            "audio/wav"
        );
        assert_eq!(
            content_type(b"\x1aE\xdf\xa3audio", "audio/webm").unwrap(),
            "audio/webm"
        );
    }
}
