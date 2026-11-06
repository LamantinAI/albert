use std::{
    ffi::CString, fs::File, fs::OpenOptions, io::Read, io::Write, os::fd::AsRawFd,
    os::fd::FromRawFd, path::Component, path::Path,
};

use libc::{openat, O_CLOEXEC, O_DIRECTORY, O_NOFOLLOW, O_NONBLOCK, O_RDONLY};

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
    let name = format!("media-{id}");
    let root = root.canonicalize().map_err(|_| MediaError::Workspace)?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(root.join(&name))
        .map_err(|_| MediaError::Workspace)?;
    file.write_all(bytes).map_err(|_| MediaError::Workspace)?;
    Ok(name)
}
