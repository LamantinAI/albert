use std::path::{Path, PathBuf};

use crate::error::{Error, Result};

pub(super) fn default_backup_dir() -> String {
    "state/history-backups".into()
}

/// Resolve the actual internal database once, independently of the process cwd.
pub(crate) fn sqlite_path(spec: Option<&str>, deploy_dir: &Path) -> Result<PathBuf> {
    let spec = spec.unwrap_or("sqlite:state/history.db");
    let path = spec.strip_prefix("sqlite:").filter(|path| !path.is_empty()).ok_or_else(|| {
        Error::Config("Session startup requires retained SQLite history: set [history] backend = \"sqlite:state/history.db\". Existing memory/file stores are not automatically converted.".into())
    })?;
    if path == ":memory:" || path.contains('?') || path.contains('\0') {
        return Err(Error::Config(
            "History requires a SQLite file path, without URI options.".into(),
        ));
    }
    let path = PathBuf::from(path);
    Ok(if path.is_absolute() {
        path
    } else {
        deploy_dir.join(path)
    })
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::sqlite_path;

    #[test]
    fn database_paths_follow_deployment_and_reject_unsupported_retention() {
        let base = Path::new("/deployment");
        assert_eq!(
            sqlite_path(None, base).unwrap(),
            base.join("state/history.db")
        );
        assert_eq!(
            sqlite_path(Some("sqlite:custom/store.db"), base).unwrap(),
            base.join("custom/store.db")
        );
        assert_eq!(
            sqlite_path(Some("sqlite:/external/history.db"), base).unwrap(),
            Path::new("/external/history.db")
        );
        for unsupported in ["memory", "file:history", "sqlite:", "sqlite::memory:"] {
            assert!(sqlite_path(Some(unsupported), base).is_err());
        }
    }
}
