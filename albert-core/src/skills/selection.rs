use std::{fs::File, io::Read, path::Path};

use sha2::{Digest, Sha256};

use super::{discovery::body_of, discovery::bundle_files, discovery::meta_of, SkillStore};
use crate::composer::{ComposerSettings, PreparedSkills, SkillChoice, SkillSelection, SkillStamp};

fn document(path: &Path) -> Result<String, String> {
    let mut bytes = Vec::new();
    File::open(path)
        .map_err(|_| "The selected skill is no longer enabled.".to_owned())?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "The selected skill could not be read.".to_owned())?;
    if bytes.len() > 1024 * 1024 {
        return Err("Skill instructions exceed 1 MiB.".into());
    }
    String::from_utf8(bytes).map_err(|_| "Skill instructions must be UTF-8.".into())
}
fn revision(text: &str) -> String {
    let mut digest = Sha256::new();
    for part in [text.as_bytes(), b"enabled"] {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    format!("{:x}", digest.finalize())
}
impl SkillStore {
    pub fn composer_choices(&self) -> Vec<SkillChoice> {
        let inner = self.inner.lock().unwrap();
        inner
            .catalog
            .iter()
            .filter(|meta| meta.standing.is_none())
            .filter_map(|meta| {
                let text = document(&meta.dir.join("SKILL.md")).ok()?;
                let front = meta_of(&text);
                if front.always
                    || front
                        .requires
                        .as_deref()
                        .is_some_and(|req| !inner.capabilities.iter().any(|cap| cap == req))
                {
                    return None;
                }
                let id = meta.dir.file_name()?.to_str()?.to_owned();
                if id.is_empty()
                    || id.len() > 64
                    || !id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_'))
                {
                    return None;
                }
                Some(SkillChoice {
                    command: meta.command.clone().filter(|command| {
                        front
                            .command
                            .as_deref()
                            .map(|value| value.trim().trim_start_matches('/'))
                            == Some(command.as_str())
                    }),
                    id,
                    name: front.name.unwrap_or_else(|| meta.name.clone()),
                    description: front.description.unwrap_or_else(|| meta.when.clone()),
                    revision: revision(&text),
                })
            })
            .collect()
    }
    pub fn prepare_composer(
        &self,
        selections: &[SkillSelection],
        settings: &ComposerSettings,
    ) -> Result<PreparedSkills, String> {
        if selections.len() > settings.max_selected_skills {
            return Err("Too many selected skills.".into());
        }
        let choices = self.composer_choices();
        let mut prepared = PreparedSkills::default();
        let mut total = 0;
        for selection in selections {
            if prepared
                .selections
                .iter()
                .any(|entry| entry.id == selection.id)
            {
                return Err("A skill may only be selected once.".into());
            }
            let choice = choices
                .iter()
                .find(|choice| choice.id == selection.id)
                .ok_or("The selected skill is unavailable. Reload the skill picker.")?;
            let directory = {
                let inner = self.inner.lock().unwrap();
                inner
                    .catalog
                    .iter()
                    .find(|meta| {
                        meta.dir.file_name().and_then(|id| id.to_str())
                            == Some(selection.id.as_str())
                    })
                    .map(|meta| meta.dir.clone())
                    .ok_or("The selected skill is unavailable.")?
            };
            let text = document(&directory.join("SKILL.md"))?;
            if choice.revision != selection.revision || revision(&text) != selection.revision {
                return Err("The selected skill changed. Remove it and select it again.".into());
            }
            let body = body_of(&text).trim();
            let instruction=format!("--- skill {:?} ---\n{body}\n\nBundled resources (use skill_file with this skill): {}",choice.name,bundle_files(&directory).join(", "));
            total += instruction.len();
            if body.is_empty() || total > settings.max_skill_instruction_bytes {
                return Err("Selected skill instructions exceed the composer limit.".into());
            }
            prepared.instructions.push(instruction);
            prepared.stamps.push(SkillStamp {
                id: choice.id.clone(),
                name: choice.name.clone(),
                revision: choice.revision.clone(),
            });
            prepared.selections.push(selection.clone());
        }
        Ok(prepared)
    }
}
