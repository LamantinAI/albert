//! Restricted, atomic control-plane files. No absolute paths leave these helpers.
use std::{
    fs::{
        canonicalize, create_dir, remove_file, rename, set_permissions, symlink_metadata, File,
        OpenOptions, Permissions,
    },
    io::{ErrorKind, Read, Write},
    os::unix::fs::{MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Component, Path, PathBuf},
    sync::Arc,
};

use axum::http::StatusCode;
use libc::O_NOFOLLOW;
use rand::random;
use sha2::{Digest, Sha256};
use tokio::{sync::Mutex, task::spawn_blocking};

use crate::api::errors::{ApiError, ApiResult};

/// The blocking job owns its lease even if the HTTP caller disconnects. Dropping
/// only the JoinHandle must never unlock a still-running file transaction.
pub(crate) async fn serialized<T, F>(gate: Arc<Mutex<()>>, job: F) -> ApiResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> ApiResult<T> + Send + 'static,
{
    let guard = gate.lock_owned().await;
    spawn_blocking(move || {
        let _guard = guard;
        job()
    })
    .await
    .map_err(|_| ApiError::internal())?
}

pub(crate) fn root(directory: &Path) -> ApiResult<PathBuf> {
    canonicalize(directory).map_err(|_| ApiError::internal())
}

pub(crate) fn checked(root: &Path, candidate: &Path) -> ApiResult<PathBuf> {
    let relative = if candidate.is_absolute() {
        candidate.strip_prefix(root).map_err(|_| {
            ApiError::invalid("Control files must remain inside the deployment directory.")
        })?
    } else {
        candidate
    };
    let mut path = root.to_path_buf();
    for component in relative.components() {
        match component {
            Component::CurDir => continue,
            Component::Normal(name) => path.push(name),
            _ => return Err(ApiError::invalid("Invalid control file path.")),
        }
        match symlink_metadata(&path) {
            Ok(metadata)
                if metadata.file_type().is_symlink()
                    || (metadata.is_file() && metadata.nlink() > 1) =>
            {
                return Err(ApiError::invalid("Linked control files are not supported."))
            }
            Ok(metadata) if !metadata.is_file() && !metadata.is_dir() => {
                return Err(ApiError::invalid("Unsupported control file type."))
            }
            Ok(_) => {}
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(_) => return Err(ApiError::internal()),
        }
    }
    if path == root {
        return Err(ApiError::invalid("A control file name is required."));
    }
    Ok(path)
}

pub(crate) fn read_optional(
    root: &Path,
    path: &Path,
    maximum: usize,
) -> ApiResult<Option<Vec<u8>>> {
    let path = checked(root, path)?;
    match symlink_metadata(&path) {
        Ok(metadata) if !metadata.is_file() || metadata.len() > maximum as u64 => Err(
            ApiError::invalid("The control file is unsupported or exceeds its size limit."),
        ),
        Ok(_) => {
            let mut bytes = Vec::new();
            OpenOptions::new()
                .read(true)
                .custom_flags(O_NOFOLLOW)
                .open(&path)
                .map_err(|_| ApiError::internal())?
                .take(maximum as u64 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| ApiError::internal())?;
            if bytes.len() > maximum {
                return Err(ApiError::invalid(
                    "The control file exceeds its size limit.",
                ));
            }
            Ok(Some(bytes))
        }
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(_) => Err(ApiError::internal()),
    }
}

pub(crate) fn ensure_directory(root: &Path, path: &Path) -> ApiResult<()> {
    ensure_directory_mode(root, path, 0o700)
}

pub(crate) fn ensure_directory_mode(root: &Path, path: &Path, mode: u32) -> ApiResult<()> {
    let target = checked(root, path)?;
    let relative = target
        .strip_prefix(root)
        .map_err(|_| ApiError::internal())?;
    let mut current = root.to_path_buf();
    for name in relative.components() {
        current.push(name);
        match create_dir(&current) {
            Ok(()) => set_permissions(&current, Permissions::from_mode(mode))
                .map_err(|_| ApiError::internal())?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => {
                if !symlink_metadata(&current)
                    .is_ok_and(|metadata| metadata.is_dir() && !metadata.file_type().is_symlink())
                {
                    return Err(ApiError::invalid("A control directory is unavailable."));
                }
            }
            Err(_) => return Err(ApiError::internal()),
        }
    }
    Ok(())
}

pub(crate) fn atomic_write(root: &Path, path: &Path, bytes: &[u8]) -> ApiResult<()> {
    atomic_write_modes(root, path, bytes, 0o600, 0o700)
}

pub(crate) fn atomic_write_modes(
    root: &Path,
    path: &Path,
    bytes: &[u8],
    mode: u32,
    directory_mode: u32,
) -> ApiResult<()> {
    let target = checked(root, path)?;
    let parent = target.parent().ok_or_else(ApiError::internal)?;
    if parent != root {
        ensure_directory_mode(root, parent, directory_mode)?;
    }
    let temporary = parent.join(format!(".albert-control-{:032x}.tmp", random::<u128>()));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .custom_flags(O_NOFOLLOW)
            .mode(mode)
            .open(&temporary)
            .map_err(|_| ApiError::internal())?;
        file.set_permissions(Permissions::from_mode(mode))
            .map_err(|_| ApiError::internal())?;
        file.write_all(bytes).map_err(|_| ApiError::internal())?;
        file.sync_all().map_err(|_| ApiError::internal())?;
        checked(root, &target)?;
        rename(&temporary, &target).map_err(|_| ApiError::internal())?;
        File::open(parent)
            .and_then(|directory| directory.sync_all())
            .map_err(|_| ApiError::internal())
    })();
    if result.is_err() {
        let _ = remove_file(temporary);
    }
    result
}

pub(crate) fn hash(parts: &[&[u8]]) -> String {
    let mut digest = Sha256::new();
    for part in parts {
        digest.update((part.len() as u64).to_be_bytes());
        digest.update(part);
    }
    format!("{:x}", digest.finalize())
}

pub(crate) fn check_revision(expected: Option<&str>, actual: &str) -> ApiResult<()> {
    if expected.is_some_and(|expected| expected != actual) {
        Err(ApiError::new(
            StatusCode::PRECONDITION_FAILED,
            "stale_revision",
            "The resource changed. Reload and retry.",
        ))
    } else {
        Ok(())
    }
}
