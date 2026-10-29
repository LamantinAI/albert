use std::{
    fs::{read_dir, symlink_metadata},
    os::unix::fs::MetadataExt,
    path::{Path, PathBuf},
    str::from_utf8,
};

use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::SkillView,
    },
    config::Config,
    service::panel_models::files::{checked, ensure_directory_mode, hash, read_optional, root},
    skills::control::inspect_document,
};

pub(super) struct Package {
    pub id: String,
    pub folder: PathBuf,
    pub path: PathBuf,
    pub content: String,
    pub enabled: bool,
    pub revision: String,
}

impl Package {
    pub fn view(&self, full: bool) -> ApiResult<SkillView> {
        let metadata = inspect_document(&self.content, &self.id).map_err(ApiError::invalid)?;
        Ok(SkillView {
            id: self.id.clone(),
            name: metadata.name,
            description: metadata.description,
            command: metadata.command,
            owner_only: metadata.owner_only,
            enabled: self.enabled,
            content: full.then(|| self.content.clone()),
            revision: self.revision.clone(),
        })
    }
}

pub(super) fn valid_id(id: &str) -> ApiResult<()> {
    if id.is_empty()
        || id.len() > 64
        || !id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_'))
    {
        Err(ApiError::invalid(
            "Skill IDs must contain 1..64 ASCII letters, digits, '-' or '_'.",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn command_available(
    root: &Path,
    id: &str,
    content: &str,
    enabled: bool,
) -> ApiResult<()> {
    let metadata = inspect_document(content, id).map_err(ApiError::invalid)?;
    if !enabled {
        return Ok(());
    }
    for entry in read_dir(root).map_err(|_| ApiError::internal())? {
        let entry = entry.map_err(|_| ApiError::internal())?;
        let Some(other) = entry.file_name().to_str().map(str::to_owned) else {
            continue;
        };
        if other == id || valid_id(&other).is_err() {
            continue;
        }
        if let Some(package) = package(root, &other)? {
            if !package.enabled {
                continue;
            }
            let existing = inspect_document(&package.content, &other).map_err(ApiError::invalid)?;
            if existing.name == metadata.name {
                return Err(ApiError::conflict(
                    "This name is already used by another enabled skill.",
                ));
            }
            if metadata.command.is_some() && existing.command == metadata.command {
                return Err(ApiError::conflict(
                    "This command is already declared by another enabled skill.",
                ));
            }
        }
    }
    Ok(())
}

pub(super) fn directory(base: &Config, create: bool) -> ApiResult<PathBuf> {
    let deploy = root(&base.deploy_dir)?;
    let path = if base.skills_dir.is_absolute() {
        base.skills_dir.clone()
    } else {
        deploy.join(&base.skills_dir)
    };
    let path = checked(Path::new("/"), &path)?;
    let state = deploy.join("state");
    if deploy.starts_with(&path)
        || path.starts_with(&state)
        || state.starts_with(&path)
        || base.subscription_auth_json.starts_with(&path)
        || base.history.as_ref().is_some_and(|history| {
            let history = history.strip_prefix("sqlite://").unwrap_or(history);
            let history = Path::new(history);
            let history = if history.is_absolute() {
                history.to_owned()
            } else {
                deploy.join(history)
            };
            history.starts_with(&path)
        })
    {
        return Err(ApiError::invalid(
            "The skills directory overlaps protected application state.",
        ));
    }
    if create {
        ensure_directory_mode(Path::new("/"), &path, 0o750)?;
    }
    Ok(path)
}

pub(super) fn package(root: &Path, id: &str) -> ApiResult<Option<Package>> {
    valid_id(id)?;
    let folder = checked(root, &root.join(id))?;
    if !folder.exists() {
        return Ok(None);
    }
    if !folder.is_dir() {
        return Err(ApiError::invalid(
            "The skill resource is not a package directory.",
        ));
    }
    let normal = read_optional(root, &folder.join("SKILL.md"), 1024 * 1024)?;
    let disabled = read_optional(root, &folder.join("SKILL.disabled.md"), 1024 * 1024)?;
    let (bytes, enabled, name) = match (normal, disabled) {
        (Some(bytes), None) => (bytes, true, "SKILL.md"),
        (None, Some(bytes)) => (bytes, false, "SKILL.disabled.md"),
        (None, None) => return Ok(None),
        _ => {
            return Err(ApiError::conflict(
                "A package cannot contain both enabled and disabled instruction files.",
            ))
        }
    };
    inspect(&folder)?;
    let content = from_utf8(&bytes)
        .map_err(|_| ApiError::invalid("Skill instructions must be UTF-8."))?
        .to_owned();
    let revision = hash(&[&bytes, if enabled { b"enabled" } else { b"disabled" }]);
    Ok(Some(Package {
        id: id.into(),
        path: folder.join(name),
        folder,
        content,
        enabled,
        revision,
    }))
}

pub(super) fn check_replacement_size(package: &Package, content: &str) -> ApiResult<()> {
    let mut bytes = 0;
    walk(&package.folder, 0, &mut 0, &mut bytes)?;
    if bytes
        .saturating_sub(package.content.len() as u64)
        .saturating_add(content.len() as u64)
        > 10 * 1024 * 1024
    {
        return Err(ApiError::invalid(
            "The skill package exceeds its size limit.",
        ));
    }
    Ok(())
}

pub(super) fn inspect(directory: &Path) -> ApiResult<()> {
    walk(directory, 0, &mut 0, &mut 0)
}

fn walk(path: &Path, depth: usize, entries: &mut usize, bytes: &mut u64) -> ApiResult<()> {
    *entries += 1;
    if depth > 32 || *entries > 256 {
        return Err(ApiError::invalid(
            "The skill package exceeds its depth or file limit.",
        ));
    }
    let metadata = symlink_metadata(path).map_err(|_| ApiError::internal())?;
    if metadata.file_type().is_symlink() || (metadata.is_file() && metadata.nlink() > 1) {
        return Err(ApiError::invalid(
            "Linked files are not allowed in skill packages.",
        ));
    }
    if metadata.is_dir() {
        for entry in read_dir(path).map_err(|_| ApiError::internal())? {
            walk(
                &entry.map_err(|_| ApiError::internal())?.path(),
                depth + 1,
                entries,
                bytes,
            )?;
        }
    } else if metadata.is_file() {
        *bytes = bytes.saturating_add(metadata.len());
        if *bytes > 10 * 1024 * 1024 {
            return Err(ApiError::invalid(
                "The skill package exceeds its size limit.",
            ));
        }
    } else {
        return Err(ApiError::invalid("Unsupported package file type."));
    }
    Ok(())
}
