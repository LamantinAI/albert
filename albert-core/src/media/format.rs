use octo_history::Turn;
use serde::{Deserialize, Serialize};
use serde_json::{from_str, to_string, to_vec};
use sha2::{Digest, Sha256};

use super::Asset;
use crate::composer::SkillStamp;

pub const MEDIA_MARKER: &str = "[albert media v1]\n";
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MediaRecord {
    pub text: String,
    pub attachments: Vec<Asset>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<SkillStamp>,
}
pub fn encode(text: String, attachments: Vec<Asset>) -> String {
    encode_selected(text, attachments, Vec::new())
}
pub fn encode_selected(text: String, attachments: Vec<Asset>, skills: Vec<SkillStamp>) -> String {
    if attachments.is_empty() && skills.is_empty() && !text.starts_with(MEDIA_MARKER) {
        return text;
    }
    format!(
        "{MEDIA_MARKER}{}",
        to_string(&MediaRecord {
            text,
            attachments,
            skills
        })
        .expect("media serializes")
    )
}
pub fn decode(text: &str) -> Option<MediaRecord> {
    text.strip_prefix(MEDIA_MARKER)
        .and_then(|value| from_str(value).ok())
}
pub fn project(text: &str) -> String {
    let Some(record) = decode(text) else {
        return text.into();
    };
    let mut text = record.text;
    if !record.skills.is_empty() {
        text.push_str(&format!(
            "\n[User-selected installed skills: {}]",
            record
                .skills
                .iter()
                .map(|skill| format!("{:?}", skill.name))
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    for asset in record.attachments {
        text.push_str(&format!(
            "\n[Attached file: {:?}, {}, {} bytes]",
            asset.filename, asset.content_type, asset.size
        ));
    }
    text
}
pub fn input_digest(text: &str, ids: &[String]) -> String {
    if ids.is_empty() {
        format!("{:x}", Sha256::digest(text.as_bytes()))
    } else {
        format!(
            "{:x}",
            Sha256::digest(to_vec(&(text, ids)).expect("input serializes"))
        )
    }
}
pub(crate) fn assets(turns: &[Turn]) -> Vec<Asset> {
    turns
        .iter()
        .filter_map(|t| decode(&t.content))
        .flat_map(|r| r.attachments)
        .collect()
}
