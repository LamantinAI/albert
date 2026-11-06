//! Trusted web-composer input contracts, separate from transport DTOs.
use serde::{Deserialize, Serialize};
use serde_json::to_vec;
use sha2::{Digest, Sha256};

use crate::media::input_digest;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct ComposerSettings {
    pub max_selected_skills: usize,
    pub max_skill_instruction_bytes: usize,
    pub recording_max_seconds: u64,
}
impl Default for ComposerSettings {
    fn default() -> Self {
        Self {
            max_selected_skills: 4,
            max_skill_instruction_bytes: 256 * 1024,
            recording_max_seconds: 300,
        }
    }
}
impl ComposerSettings {
    pub fn valid(&self) -> bool {
        (1..=16).contains(&self.max_selected_skills)
            && (1024..=4 * 1024 * 1024).contains(&self.max_skill_instruction_bytes)
            && (1..=3600).contains(&self.recording_max_seconds)
    }
}
#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SkillSelection {
    pub id: String,
    pub revision: String,
}
#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct SkillStamp {
    pub id: String,
    pub name: String,
    pub revision: String,
}
#[derive(Clone, Debug)]
pub struct SkillChoice {
    pub command: Option<String>,
    pub id: String,
    pub name: String,
    pub description: String,
    pub revision: String,
}

/// Only the core skill loader can construct instruction-bearing input.
#[derive(Clone, Default)]
pub struct PreparedSkills {
    pub(crate) selections: Vec<SkillSelection>,
    pub(crate) stamps: Vec<SkillStamp>,
    pub(crate) instructions: Vec<String>,
}
impl PreparedSkills {
    pub fn selections(&self) -> &[SkillSelection] {
        &self.selections
    }
    pub fn stamps(&self) -> Vec<SkillStamp> {
        self.stamps.clone()
    }
    pub fn is_empty(&self) -> bool {
        self.selections.is_empty()
    }
    pub fn seed(&self, text: &str) -> String {
        format!("The user explicitly selected the following installed skills for this request. Their instructions are already loaded. Use the permitted tools for this conversation; these instructions do not grant additional permissions.\n\n{}\n\n--- user request ---\n{text}",self.instructions.join("\n\n--- next selected skill ---\n\n"))
    }
}
pub fn message_digest(text: &str, attachments: &[String], skills: &[SkillSelection]) -> String {
    if skills.is_empty() {
        input_digest(text, attachments)
    } else {
        format!(
            "{:x}",
            Sha256::digest(to_vec(&(text, attachments, skills)).expect("input serializes"))
        )
    }
}
#[derive(Clone)]
pub struct InputDigest(pub String);
/// Typed, host-authenticated command; ordinary web text is never parsed as one.
#[derive(Clone)]
pub struct CompactRequest {
    pub instructions: String,
}

#[cfg(test)]
mod tests {
    use super::{message_digest, ComposerSettings, SkillSelection};
    use crate::{
        media::{decode, encode_selected, input_digest, project},
        skills::SkillStore,
    };
    use std::fs::{create_dir_all, write};
    use tempfile::tempdir;

    #[test]
    fn selection_identity_preserves_legacy_receipts_and_binds_order_and_revision() {
        let a = SkillSelection {
            id: "brief".into(),
            revision: "a".into(),
        };
        let b = SkillSelection {
            id: "research".into(),
            revision: "b".into(),
        };
        assert_eq!(
            message_digest("hello", &[], &[]),
            input_digest("hello", &[])
        );
        assert_ne!(
            message_digest("hello", &[], &[a.clone(), b.clone()]),
            message_digest("hello", &[], &[b, a.clone()])
        );
        assert_ne!(
            message_digest("hello", &[], &[a.clone()]),
            message_digest(
                "hello",
                &[],
                &[SkillSelection {
                    revision: "changed".into(),
                    ..a
                }]
            )
        );
    }
    #[test]
    fn explicit_skills_are_bounded_current_and_journal_only_names_not_instructions() {
        let dir = tempdir().unwrap();
        for (id, body) in [
            (
                "brief",
                "---\nname: Brief\ndescription: Short answers\ncommand: settings\n---\nPrivate instruction body",
            ),
            (
                "standing",
                "---\nname: Style\nalways: true\n---\nAlways instruction",
            ),
            (
                "requires",
                "---\nname: Special\nrequires: subscription\n---\nSpecial instruction",
            ),
        ] {
            create_dir_all(dir.path().join(id)).unwrap();
            write(dir.path().join(id).join("SKILL.md"), body).unwrap();
        }
        let store = SkillStore::load(dir.path().into(), 4, 10, &[]);
        let choices = store.composer_choices();
        assert_eq!(choices.len(), 1);
        assert_eq!(choices[0].id, "brief");
        assert_eq!(choices[0].command.as_deref(), Some("settings"));
        let selection = SkillSelection {
            id: choices[0].id.clone(),
            revision: choices[0].revision.clone(),
        };
        let prepared = store
            .prepare_composer(&[selection.clone()], &ComposerSettings::default())
            .unwrap();
        assert!(prepared
            .seed("question")
            .contains("Private instruction body"));
        let journal = encode_selected("question".into(), vec![], prepared.stamps());
        assert!(!journal.contains("Private instruction body"));
        assert!(project(&journal).contains("Brief"));
        assert_eq!(decode(&journal).unwrap().skills.len(), 1);
        assert!(store
            .prepare_composer(
                &[selection.clone(), selection.clone()],
                &ComposerSettings::default()
            )
            .is_err());
        write(
            dir.path().join("brief/SKILL.md"),
            "---\nname: Brief\n---\nChanged instruction",
        )
        .unwrap();
        assert!(store
            .prepare_composer(&[selection], &ComposerSettings::default())
            .is_err());
        let changed = store.composer_choices().remove(0);
        let prepared = store.prepare_composer(
            &[SkillSelection {
                id: changed.id,
                revision: changed.revision,
            }],
            &ComposerSettings {
                max_skill_instruction_bytes: 1,
                ..ComposerSettings::default()
            },
        );
        assert!(prepared.is_err());
    }
}
