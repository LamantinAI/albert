#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::{
    fs::{create_dir_all, hard_link, read, remove_file, OpenOptions},
    io::{ErrorKind, Write},
    path::Path,
};

use rand::{rngs::OsRng, RngCore};

use super::{random_id, AuthError};

fn read_checked(path: &Path) -> Result<Vec<u8>, AuthError> {
    let bytes = read(path)?;
    if bytes.len() != 32 {
        return Err(AuthError::Configuration);
    }
    #[cfg(unix)]
    if path.metadata()?.permissions().mode() & 0o077 != 0 {
        return Err(AuthError::Configuration);
    }
    Ok(bytes)
}

pub(super) fn load_or_create(path: &Path) -> Result<Vec<u8>, AuthError> {
    match read_checked(path) {
        Ok(bytes) => return Ok(bytes),
        Err(AuthError::Io(error)) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    let parent = path.parent().ok_or(AuthError::Configuration)?;
    create_dir_all(parent)?;
    let temporary = parent.join(format!(".web-key-{}.tmp", random_id()));
    let mut bytes = vec![0u8; 32];
    OsRng.fill_bytes(&mut bytes);
    let result = (|| {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options.mode(0o600);
        let mut file = options.open(&temporary)?;
        file.write_all(&bytes)?;
        file.sync_all()?;
        // Publish a complete key only, without overwriting a concurrent winner.
        match hard_link(&temporary, path) {
            Ok(()) => Ok(bytes),
            Err(error) if error.kind() == ErrorKind::AlreadyExists => read_checked(path),
            Err(error) => Err(AuthError::Io(error)),
        }
    })();
    let _ = remove_file(temporary);
    result
}
