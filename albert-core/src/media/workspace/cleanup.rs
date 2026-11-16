use std::{
    ffi::CString, fs::OpenOptions, io::Error as IoError, io::ErrorKind, os::fd::AsRawFd,
    os::unix::fs::OpenOptionsExt, path::Path,
};

use libc::{unlinkat, O_CLOEXEC, O_DIRECTORY, O_NOFOLLOW};
use octo_history::SessionId;

use super::super::{MediaError, MediaResult};

/// Remove one reserved attachment-copy name without following a symlink or
/// recursively erasing a directory. The original root must remain available.
pub fn discard_input_file(root: &Path, id: &str) -> MediaResult<()> {
    let id = SessionId::parse(id).map_err(|_| MediaError::Workspace)?;
    let directory = OpenOptions::new()
        .read(true)
        .custom_flags(O_CLOEXEC | O_DIRECTORY | O_NOFOLLOW)
        .open(root)
        .map_err(|_| MediaError::Workspace)?;
    let name = CString::new(format!("media-{id}")).map_err(|_| MediaError::Workspace)?;
    // SAFETY: name is a validated single component and directory is an owned,
    // non-symlink directory descriptor. Flags never recurse or follow targets.
    let removed = unsafe { unlinkat(directory.as_raw_fd(), name.as_ptr(), 0) };
    if removed < 0 && IoError::last_os_error().kind() != ErrorKind::NotFound {
        return Err(MediaError::Workspace);
    }
    directory.sync_all().map_err(|_| MediaError::Workspace)
}
