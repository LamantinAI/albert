//! Local payload storage. The same relative key works under any configured root.
use std::{
    fs::{
        create_dir_all, hard_link, remove_dir_all, remove_file, symlink_metadata, DirBuilder, File,
        OpenOptions,
    },
    io::{ErrorKind, Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

use libc::{O_NOFOLLOW, O_NONBLOCK};
use octo_history::SessionId;
use sha2::{Digest, Sha256};
use sqlx::{query_scalar, SqlitePool};

use super::{MediaError, MediaResult};

#[derive(Clone)]
pub struct AttachmentFiles(PathBuf);

impl AttachmentFiles {
    pub fn new(root: PathBuf) -> Self {
        Self(root)
    }

    pub(crate) async fn beside_database(pool: &SqlitePool) -> Result<Option<Self>, sqlx::Error> {
        let path: String = query_scalar("SELECT file FROM pragma_database_list WHERE name='main'")
            .fetch_one(pool)
            .await?;
        Ok((!path.is_empty()).then(|| Self(PathBuf::from(format!("{path}.attachments")))))
    }

    fn session_dir(&self, session: &str) -> MediaResult<PathBuf> {
        Ok(self.0.join(component(session)?))
    }

    pub(crate) fn store(&self, session: &str, id: &str, bytes: &[u8]) -> MediaResult<String> {
        let hash = format!("{:x}", Sha256::digest(bytes));
        let directory = self.session_dir(session)?;
        let target = directory.join(component(id)?);
        create_dir_all(&self.0)?;
        match DirBuilder::new().mode(0o700).create(&directory) {
            Ok(()) => File::open(&self.0)?.sync_all()?,
            Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
            Err(error) => return Err(error.into()),
        }
        check_directory(&directory)?;
        if target.symlink_metadata().is_ok() {
            verify(&target, bytes.len(), &hash)?;
            File::open(&directory)?.sync_all()?;
            return Ok(hash);
        }
        let staging = directory.join(format!(".upload-{}", SessionId::new()));
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&staging)?;
        let result = (|| {
            file.write_all(bytes)?;
            file.sync_all()?;
            // An interrupted attempt can leave the complete payload behind. Never
            // replace an existing object under an immutable attachment ID.
            match hard_link(&staging, &target) {
                Ok(()) => (),
                Err(error) if error.kind() == ErrorKind::AlreadyExists => (),
                Err(error) => return Err(error.into()),
            }
            verify(&target, bytes.len(), &hash)?;
            Ok::<(), MediaError>(())
        })();
        let cleanup = remove_file(&staging);
        result?;
        cleanup?;
        File::open(&directory)?.sync_all()?;
        Ok(hash)
    }

    pub(crate) fn slice(
        &self,
        session: &str,
        id: &str,
        size: usize,
        offset: usize,
        len: usize,
    ) -> MediaResult<Vec<u8>> {
        if offset.checked_add(len).is_none_or(|end| end > size) {
            return Err(MediaError::NotFound);
        }
        let directory = self.session_dir(session)?;
        check_directory(&directory)?;
        let mut file = open_payload(&directory.join(component(id)?), size)?;
        file.seek(SeekFrom::Start(offset as u64))?;
        let mut bytes = vec![0; len];
        file.read_exact(&mut bytes)?;
        Ok(bytes)
    }

    pub(crate) fn discard_session(&self, session: &str) -> MediaResult<()> {
        let directory = self.session_dir(session)?;
        match remove_dir_all(directory) {
            Ok(()) => File::open(&self.0)?.sync_all()?,
            Err(error) if error.kind() == ErrorKind::NotFound => (),
            Err(error) => return Err(error.into()),
        }
        Ok(())
    }
}

fn component(value: &str) -> MediaResult<&str> {
    let id = SessionId::parse(value).map_err(|_| MediaError::StorageIntegrity)?;
    if id.as_str() != value {
        return Err(MediaError::StorageIntegrity);
    }
    Ok(value)
}

fn check_directory(path: &Path) -> MediaResult<()> {
    if !symlink_metadata(path)?.file_type().is_dir() {
        return Err(MediaError::StorageIntegrity);
    }
    Ok(())
}

fn open_payload(path: &Path, size: usize) -> MediaResult<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(O_NOFOLLOW | O_NONBLOCK)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.len() != size as u64 {
        return Err(MediaError::StorageIntegrity);
    }
    Ok(file)
}

fn verify(path: &Path, size: usize, hash: &str) -> MediaResult<()> {
    let mut file = open_payload(path, size)?;
    let mut digest = Sha256::new();
    let mut buffer = [0; 8192];
    loop {
        let count = file.read(&mut buffer)?;
        if count == 0 {
            break;
        }
        digest.update(&buffer[..count]);
    }
    if format!("{:x}", digest.finalize()) != hash {
        return Err(MediaError::StorageIntegrity);
    }
    Ok(())
}
