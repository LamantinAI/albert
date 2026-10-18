//! A read-only look at which connector types the manifests declare, before the runtime
//! loads them — so what Albert offers (a skill's `requires:`) can follow what is actually
//! configured. The octo loader stays the one that instantiates connectors.

use std::{
    collections::HashSet,
    fs::{read_dir, read_to_string},
    path::{Path, PathBuf},
};

use toml::Value;

/// The `type`s declared under the `[connectors] dir` of the octo manifest at `octo_toml`:
/// folder-style `<dir>/<name>/<name>.toml` and flat `<dir>/*.toml`. Unreadable files are
/// skipped (the loader reports them properly).
pub fn connector_files(octo_toml: &Path) -> Vec<PathBuf> {
    let base = octo_toml.parent().unwrap_or(Path::new("."));
    let value = read_to_string(octo_toml)
        .ok()
        .and_then(|s| s.parse::<Value>().ok());
    let dir = value
        .as_ref()
        .and_then(|v| v.get("connectors")?.get("dir")?.as_str())
        .unwrap_or("connectors");
    let mut files: Vec<_> = read_dir(base.join(dir))
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|e| {
            let path = e.path();
            if path.is_dir() {
                let name = path.file_name()?.to_str()?;
                Some(path.join(format!("{name}.toml")))
            } else {
                (path.extension()?.to_str()? == "toml").then_some(path)
            }
        })
        .collect();
    if let Some(explicit) = value
        .as_ref()
        .and_then(|v| v.get("connectors")?.get("explicit")?.as_array())
    {
        files.extend(
            explicit
                .iter()
                .filter_map(|v| v.get("path")?.as_str())
                .map(|p| base.join(p)),
        );
    }
    files.sort();
    files.dedup();
    files
}

pub fn declared_types(octo_toml: &Path) -> HashSet<String> {
    connector_files(octo_toml)
        .into_iter()
        .filter_map(|file| {
            let value: Value = read_to_string(file).ok()?.parse().ok()?;
            value
                .get("connector")?
                .get("type")?
                .as_str()
                .map(str::to_string)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::declared_types;
    use std::fs::{create_dir_all, write};

    #[test]
    fn folder_and_flat_manifests_are_both_seen() {
        let root = tempfile::tempdir().unwrap();
        write(
            root.path().join("octo.toml"),
            "[connectors]\ndir = \"connectors\"\n",
        )
        .unwrap();
        let dir = root.path().join("connectors");
        create_dir_all(dir.join("imagegen")).unwrap();
        write(
            dir.join("imagegen/imagegen.toml"),
            "[connector]\nid = \"imagegen\"\ntype = \"imagegen\"\n",
        )
        .unwrap();
        write(
            dir.join("search.toml"),
            "[connector]\nid = \"search\"\ntype = \"search\"\n",
        )
        .unwrap();
        create_dir_all(dir.join("calendar")).unwrap(); // a folder with no manifest
        let types = declared_types(&root.path().join("octo.toml"));
        assert!(types.contains("imagegen") && types.contains("search"));
        assert_eq!(types.len(), 2);
    }
}
