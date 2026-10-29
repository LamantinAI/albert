use std::{
    collections::HashSet,
    ffi::CString,
    fs::{remove_dir_all, rename, File},
    io::{Cursor, Read},
    os::unix::ffi::OsStrExt,
    path::{Component, Path, PathBuf},
    str::from_utf8,
};

use base64::{engine::general_purpose::STANDARD, Engine};
use libc::{renameat2, AT_FDCWD, RENAME_EXCHANGE};
use rand::random;
use zip::ZipArchive;

use super::{
    files::{command_available, inspect, package, valid_id},
    SkillService,
};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        models::{SkillImport, SkillView},
    },
    service::panel_models::files::{atomic_write_modes, checked, ensure_directory_mode},
    skills::control::inspect_document,
};

const MAXIMUM: usize = 10 * 1024 * 1024;

struct Resource {
    path: PathBuf,
    bytes: Vec<u8>,
    executable: bool,
}

impl SkillService {
    pub(super) fn import(&self, root: &Path, input: SkillImport) -> ApiResult<SkillView> {
        if input.data_base64.len() > 8 * 1024 * 1024 {
            return Err(ApiError::invalid("The encoded skill upload exceeds 8 MiB."));
        }
        if input.filename.contains(['/', '\\', '\0']) {
            return Err(ApiError::invalid(
                "Upload a single skill file or ZIP package.",
            ));
        }
        let bytes = STANDARD
            .decode(input.data_base64)
            .map_err(|_| ApiError::invalid("Invalid base64 skill upload."))?;
        let filename = Path::new(&input.filename);
        let mut resources = match filename
            .extension()
            .and_then(|value| value.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref()
        {
            Some("md") => vec![Resource {
                path: "SKILL.md".into(),
                bytes,
                executable: false,
            }],
            Some("zip") => decode_zip(bytes)?,
            _ => {
                return Err(ApiError::invalid(
                    "Upload a Markdown skill or a ZIP package.",
                ))
            }
        };
        let metadata_paths = resources
            .iter()
            .filter(|resource| {
                matches!(
                    resource.path.file_name().and_then(|name| name.to_str()),
                    Some("SKILL.md" | "SKILL.disabled.md")
                )
            })
            .map(|resource| resource.path.clone())
            .collect::<Vec<_>>();
        if metadata_paths.len() != 1 {
            return Err(ApiError::invalid(
                "A package needs exactly one SKILL.md or SKILL.disabled.md.",
            ));
        }
        let metadata_path = &metadata_paths[0];
        let prefix = metadata_path.parent().ok_or_else(ApiError::internal)?;
        let metadata_bytes = &resources
            .iter()
            .find(|resource| resource.path == *metadata_path)
            .ok_or_else(ApiError::internal)?
            .bytes;
        if metadata_bytes.len() > 1024 * 1024 {
            return Err(ApiError::invalid("Skill instructions exceed 1 MiB."));
        }
        let text = from_utf8(metadata_bytes)
            .map_err(|_| ApiError::invalid("Skill instructions must be UTF-8."))?;
        let metadata = inspect_document(text, "skill").map_err(ApiError::invalid)?;
        let raw_id = prefix
            .file_name()
            .and_then(|name| name.to_str())
            .or_else(|| {
                filename
                    .file_stem()
                    .and_then(|name| name.to_str())
                    .filter(|name| !name.eq_ignore_ascii_case("SKILL"))
            })
            .unwrap_or(&metadata.name);
        let id = slug(raw_id);
        valid_id(&id)?;
        command_available(
            root,
            &id,
            text,
            metadata_path
                .file_name()
                .is_some_and(|name| name == "SKILL.md"),
        )?;
        for resource in &mut resources {
            resource.path = resource
                .path
                .strip_prefix(prefix)
                .map_err(|_| {
                    ApiError::invalid("Files outside the skill package are not supported.")
                })?
                .to_owned();
            if resource.path.components().count() > 32 {
                return Err(ApiError::invalid(
                    "The package exceeds its directory depth limit.",
                ));
            }
        }
        let target = checked(root, &root.join(&id))?;
        if target.exists() {
            if package(root, &id)?.is_none() {
                return Err(ApiError::conflict(
                    "The target directory is not an installed skill package.",
                ));
            }
            if !input.replace {
                return Err(ApiError::conflict(
                    "The skill already exists. Explicit replacement is required.",
                ));
            }
        }
        let staging = root.join(format!(".albert-skill-{:032x}", random::<u128>()));
        ensure_directory_mode(root, &staging, 0o750)?;
        let outcome = (|| {
            for resource in &resources {
                atomic_write_modes(
                    root,
                    &staging.join(&resource.path),
                    &resource.bytes,
                    if resource.executable { 0o750 } else { 0o640 },
                    0o750,
                )?;
            }
            inspect(&staging)?;
            checked(root, &target)?;
            if target.exists() {
                exchange(&staging, &target)?;
            } else {
                rename(&staging, &target).map_err(|_| ApiError::internal())?;
            }
            File::open(root)
                .and_then(|directory| directory.sync_all())
                .map_err(|_| ApiError::internal())?;
            package(root, &id)?
                .ok_or_else(ApiError::internal)?
                .view(true)
        })();
        if staging.exists() {
            let _ = remove_dir_all(staging);
        }
        outcome
    }
}

fn slug(value: &str) -> String {
    value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || character == '_' {
                character.to_ascii_lowercase()
            } else {
                '-'
            }
        })
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

fn decode_zip(bytes: Vec<u8>) -> ApiResult<Vec<Resource>> {
    let mut archive = ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| ApiError::invalid("Invalid ZIP package."))?;
    if archive.len() > 256 {
        return Err(ApiError::invalid("The archive contains too many entries."));
    }
    let mut resources = Vec::new();
    let mut paths = HashSet::new();
    let mut expanded = 0usize;
    for index in 0..archive.len() {
        let mut entry = archive
            .by_index(index)
            .map_err(|_| ApiError::invalid("Invalid ZIP entry."))?;
        if entry.name().contains(['\\', '\0', ':']) {
            return Err(ApiError::invalid("Invalid package resource path."));
        }
        let path = entry
            .enclosed_name()
            .ok_or_else(|| ApiError::invalid("Package paths cannot escape their directory."))?;
        if path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
            || path.components().count() > 32
        {
            return Err(ApiError::invalid("Invalid package resource path."));
        }
        let mode = entry.unix_mode().unwrap_or(0);
        if !matches!(mode & 0o170000, 0 | 0o100000 | 0o040000) {
            return Err(ApiError::invalid(
                "Links and special files are not supported in skill archives.",
            ));
        }
        if entry.is_dir() {
            continue;
        }
        if !paths.insert(path.clone()) {
            return Err(ApiError::invalid(
                "The archive contains duplicate resource paths.",
            ));
        }
        if entry.size() > (MAXIMUM - expanded) as u64 {
            return Err(ApiError::invalid("The expanded archive exceeds 10 MiB."));
        }
        let mut data = Vec::new();
        (&mut entry)
            .take((MAXIMUM - expanded + 1) as u64)
            .read_to_end(&mut data)
            .map_err(|_| ApiError::invalid("The archive could not be expanded."))?;
        expanded = expanded.saturating_add(data.len());
        if expanded > MAXIMUM {
            return Err(ApiError::invalid("The expanded archive exceeds 10 MiB."));
        }
        resources.push(Resource {
            path,
            bytes: data,
            executable: mode & 0o111 != 0,
        });
    }
    Ok(resources)
}

fn exchange(left: &Path, right: &Path) -> ApiResult<()> {
    let left = CString::new(left.as_os_str().as_bytes())
        .map_err(|_| ApiError::invalid("Invalid package path."))?;
    let right = CString::new(right.as_os_str().as_bytes())
        .map_err(|_| ApiError::invalid("Invalid package path."))?;
    // SAFETY: both pointers are NUL-terminated CStrings alive for the syscall.
    // The validated absolute sibling directories share a filesystem; exchange
    // either swaps both names atomically or leaves both unchanged.
    let result = unsafe {
        renameat2(
            AT_FDCWD,
            left.as_ptr(),
            AT_FDCWD,
            right.as_ptr(),
            RENAME_EXCHANGE,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err(ApiError::unavailable(
            "Atomic skill replacement is unavailable on this filesystem.",
        ))
    }
}
