mod cleanup;
pub use cleanup::discard_input_file;

use std::{
    ffi::CString,
    fs::File,
    fs::Permissions,
    io::Error as IoError,
    io::ErrorKind,
    io::Read,
    io::Write,
    os::fd::AsRawFd,
    os::fd::FromRawFd,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Component,
    path::Path,
};

use libc::{
    linkat, openat, unlinkat, O_CLOEXEC, O_CREAT, O_DIRECTORY, O_EXCL, O_NOFOLLOW, O_NONBLOCK,
    O_RDONLY, O_WRONLY,
};
use octo_history::SessionId;

use super::{MediaError, MediaResult};

/// Walk relative components through directory descriptors. No component may be
/// a symlink, including after concurrent workspace changes. Nonblocking opens
/// keep device files and pipes from hanging the bounded file snapshot.
pub fn read_workspace_file(root: &Path, relative: &str, limit: usize) -> MediaResult<Vec<u8>> {
    let parts = Path::new(relative)
        .components()
        .filter(|part| *part != Component::CurDir)
        .collect::<Vec<_>>();
    if parts.is_empty()
        || parts
            .iter()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(MediaError::Workspace);
    }
    let mut file = File::open(root).map_err(|_| MediaError::Workspace)?;
    for (index, part) in parts.iter().enumerate() {
        let Component::Normal(name) = part else {
            return Err(MediaError::Workspace);
        };
        let name = CString::new(name.as_encoded_bytes()).map_err(|_| MediaError::Workspace)?;
        let flags = O_RDONLY
            | O_CLOEXEC
            | O_NOFOLLOW
            | O_NONBLOCK
            | if index + 1 < parts.len() {
                O_DIRECTORY
            } else {
                0
            };
        // SAFETY: the parent descriptor and nul-terminated name remain live;
        // openat returns a newly owned descriptor, transferred exactly once.
        let fd = unsafe { openat(file.as_raw_fd(), name.as_ptr(), flags) };
        if fd < 0 {
            return Err(MediaError::Workspace);
        }
        file = unsafe { File::from_raw_fd(fd) };
    }
    let metadata = file.metadata().map_err(|_| MediaError::Workspace)?;
    if !metadata.is_file() {
        return Err(MediaError::Workspace);
    }
    if metadata.len() > limit as u64 {
        return Err(MediaError::Limit);
    }
    let mut bytes = Vec::new();
    file.take(limit as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| MediaError::Workspace)?;
    if bytes.len() > limit {
        return Err(MediaError::Limit);
    }
    Ok(bytes)
}
pub fn save_input_file(root: &Path, id: &str, bytes: &[u8]) -> MediaResult<String> {
    save_input_file_with_mode(root, id, bytes, 0o600)
}

pub(crate) fn save_script_input_file(root: &Path, id: &str, bytes: &[u8]) -> MediaResult<String> {
    save_input_file_with_mode(root, id, bytes, 0o640)
}

fn save_input_file_with_mode(
    root: &Path,
    id: &str,
    bytes: &[u8],
    mode: u32,
) -> MediaResult<String> {
    SessionId::parse(id).map_err(|_| MediaError::Workspace)?;
    let name = format!("media-{id}");
    let directory = File::open(root).map_err(|_| MediaError::Workspace)?;
    let target = CString::new(name.as_str()).map_err(|_| MediaError::Workspace)?;
    let temporary = CString::new(format!(".media-upload-{}", SessionId::new()))
        .map_err(|_| MediaError::Workspace)?;
    // SAFETY: both names are owned, nul-terminated single path components and
    // the directory descriptor remains open for every operation below.
    let fd = unsafe {
        openat(
            directory.as_raw_fd(),
            temporary.as_ptr(),
            O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC,
            0o600,
        )
    };
    if fd < 0 {
        return Err(MediaError::Workspace);
    }
    // SAFETY: openat returned a new owned file descriptor, transferred once.
    let mut file = unsafe { File::from_raw_fd(fd) };
    let outcome = (|| {
        file.write_all(bytes).map_err(|_| MediaError::Workspace)?;
        file.set_permissions(Permissions::from_mode(mode))
            .map_err(|_| MediaError::Workspace)?;
        file.sync_all().map_err(|_| MediaError::Workspace)?;
        // Publishing the complete inode with linkat is exclusive: an existing
        // workspace file, directory or symlink is never replaced or followed.
        let result = unsafe {
            linkat(
                directory.as_raw_fd(),
                temporary.as_ptr(),
                directory.as_raw_fd(),
                target.as_ptr(),
                0,
            )
        };
        if result < 0 {
            if IoError::last_os_error().kind() != ErrorKind::AlreadyExists {
                return Err(MediaError::Workspace);
            }
            let existing = unsafe {
                openat(
                    directory.as_raw_fd(),
                    target.as_ptr(),
                    O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC,
                )
            };
            if existing < 0 {
                return Err(MediaError::Workspace);
            }
            // SAFETY: the descriptor is newly owned and is transferred once.
            let mut existing = unsafe { File::from_raw_fd(existing) };
            let metadata = existing.metadata().map_err(|_| MediaError::Workspace)?;
            if !metadata.is_file() || metadata.len() != bytes.len() as u64 {
                return Err(MediaError::Workspace);
            }
            let mut buffer = [0; 8192];
            for chunk in bytes.chunks(buffer.len()) {
                existing
                    .read_exact(&mut buffer[..chunk.len()])
                    .map_err(|_| MediaError::Workspace)?;
                if &buffer[..chunk.len()] != chunk {
                    return Err(MediaError::Workspace);
                }
            }
            if existing
                .read(&mut buffer[..1])
                .map_err(|_| MediaError::Workspace)?
                != 0
            {
                return Err(MediaError::Workspace);
            }
            if mode == 0o640 && metadata.mode() & 0o040 == 0 {
                let staging_owner = file.metadata().map_err(|_| MediaError::Workspace)?;
                if metadata.nlink() != 1
                    || metadata.uid() != staging_owner.uid()
                    || metadata.gid() != staging_owner.gid()
                {
                    return Err(MediaError::Workspace);
                }
                existing
                    .set_permissions(Permissions::from_mode(mode))
                    .map_err(|_| MediaError::Workspace)?;
                existing.sync_all().map_err(|_| MediaError::Workspace)?;
            }
        }
        Ok(())
    })();
    // SAFETY: remove only the unique temporary component in the same directory.
    let removed = unsafe { unlinkat(directory.as_raw_fd(), temporary.as_ptr(), 0) };
    outcome?;
    if removed < 0 {
        return Err(MediaError::Workspace);
    }
    directory.sync_all().map_err(|_| MediaError::Workspace)?;
    Ok(name)
}

#[cfg(test)]
mod tests {
    use std::{
        fs::create_dir, fs::read, fs::read_dir, fs::symlink_metadata, fs::write,
        os::unix::fs::symlink, os::unix::fs::PermissionsExt,
    };

    use octo_history::SessionId;
    use tempfile::tempdir;

    use super::save_input_file;

    #[test]
    fn cleanup_removes_only_managed_names_and_never_follows_links_or_directories() {
        let dir = tempdir().unwrap();
        let id = SessionId::new().to_string();
        let other = dir.path().join("independent.txt");
        write(&other, "keep").unwrap();
        let managed = dir.path().join(format!("media-{id}"));
        symlink(&other, &managed).unwrap();
        super::discard_input_file(dir.path(), &id).unwrap();
        assert_eq!(read(&other).unwrap(), b"keep");
        assert!(!managed.exists());
        super::discard_input_file(dir.path(), &id).unwrap();
        create_dir(&managed).unwrap();
        write(managed.join("unrelated.txt"), "keep").unwrap();
        assert!(super::discard_input_file(dir.path(), &id).is_err());
        assert!(managed.join("unrelated.txt").exists());
        assert!(super::discard_input_file(dir.path(), "../independent.txt").is_err());
        let alias = dir.path().join("workspace-alias");
        symlink(dir.path(), &alias).unwrap();
        assert!(super::discard_input_file(&alias, &id).is_err());
        assert!(super::discard_input_file(&dir.path().join("missing-root"), &id).is_err());
    }

    #[test]
    fn input_files_are_private_atomic_idempotent_and_never_overwrite_workspace_data() {
        let dir = tempdir().unwrap();
        let id = SessionId::new().to_string();
        let bytes = vec![42; 2 * 1024 * 1024];
        let path = save_input_file(dir.path(), &id, &bytes).unwrap();
        assert_eq!(read(dir.path().join(&path)).unwrap(), bytes);
        assert_eq!(
            symlink_metadata(dir.path().join(&path))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(save_input_file(dir.path(), &id, &bytes).unwrap(), path);
        write(dir.path().join(&path), b"agent edit").unwrap();
        assert!(save_input_file(dir.path(), &id, &bytes).is_err());
        assert_eq!(read(dir.path().join(&path)).unwrap(), b"agent edit");
        assert_eq!(read_dir(dir.path()).unwrap().count(), 1);
        assert!(save_input_file(dir.path(), "../../outside", b"unsafe").is_err());
    }

    #[test]
    fn input_preparation_never_follows_symlinks_even_when_the_target_matches() {
        let dir = tempdir().unwrap();
        let id = SessionId::new().to_string();
        let root = dir.path().join("workspace");
        create_dir(&root).unwrap();
        let outside = dir.path().join("outside");
        write(&outside, b"matching bytes").unwrap();
        symlink(&outside, root.join(format!("media-{id}"))).unwrap();
        assert!(save_input_file(&root, &id, b"matching bytes").is_err());
        assert_eq!(read(outside).unwrap(), b"matching bytes");
        assert_eq!(read_dir(root).unwrap().count(), 1);
    }
}
