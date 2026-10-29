use super::discovery::{body_of, meta_of};
use crate::commands::refusal;

pub(crate) struct SkillMetadata {
    pub name: String,
    pub description: String,
    pub command: Option<String>,
    pub owner_only: bool,
}

pub(crate) fn inspect_document(text: &str, id: &str) -> Result<SkillMetadata, &'static str> {
    if text.len() > 1024 * 1024 || body_of(text).trim().is_empty() {
        return Err("A skill needs a nonempty body no larger than 1 MiB.");
    }
    let front = meta_of(text);
    let name = front.name.unwrap_or_else(|| id.into());
    let description = front.description.unwrap_or_else(|| "No description".into());
    if name.trim().is_empty() || name.len() > 128 || description.len() > 4096 {
        return Err("Invalid skill name or description.");
    }
    let command = front
        .command
        .map(|value| value.trim().trim_start_matches('/').to_owned());
    if command
        .as_ref()
        .is_some_and(|value| refusal(value).is_some())
    {
        return Err("The skill command is invalid or reserved.");
    }
    Ok(SkillMetadata {
        name,
        description,
        command,
        owner_only: front.command_owner,
    })
}
