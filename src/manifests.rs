//! A read-only look at which connector types the manifests declare, before the runtime
//! loads them — so what Albert offers (a skill's `requires:`) can follow what is actually
//! configured. The octo loader stays the one that instantiates connectors.

use std::{
    collections::HashSet,
    fs::{read_dir, read_to_string},
    path::Path,
};

use toml::Value;

/// The `type`s declared under the `[connectors] dir` of the octo manifest at `octo_toml`:
/// folder-style `<dir>/<name>/<name>.toml` and flat `<dir>/*.toml`. Unreadable files are
/// skipped (the loader reports them properly).
pub fn declared_types(octo_toml: &Path) -> HashSet<String> {
    let base = octo_toml.parent().unwrap_or(Path::new("."));
    let dir = read_to_string(octo_toml)
        .ok()
        .and_then(|t| t.parse::<Value>().ok())
        .and_then(|v| v.get("connectors")?.get("dir")?.as_str().map(str::to_string))
        .unwrap_or_else(|| "connectors".into());
    let Ok(entries) = read_dir(base.join(dir)) else {
        return HashSet::new();
    };
    entries
        .flatten()
        .map(|e| e.path())
        .filter_map(|p| {
            if p.is_dir() {
                let name = p.file_name()?.to_str()?.to_string();
                Some(p.join(format!("{name}.toml")))
            } else {
                (p.extension()? == "toml").then_some(p)
            }
        })
        .filter_map(|file| {
            let v: Value = read_to_string(file).ok()?.parse().ok()?;
            v.get("connector")?.get("type")?.as_str().map(str::to_string)
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
        write(root.path().join("octo.toml"), "[connectors]\ndir = \"connectors\"\n").unwrap();
        let dir = root.path().join("connectors");
        create_dir_all(dir.join("imagegen")).unwrap();
        write(dir.join("imagegen/imagegen.toml"), "[connector]\nid = \"imagegen\"\ntype = \"imagegen\"\n").unwrap();
        write(dir.join("search.toml"), "[connector]\nid = \"search\"\ntype = \"search\"\n").unwrap();
        create_dir_all(dir.join("calendar")).unwrap(); // a folder with no manifest
        let types = declared_types(&root.path().join("octo.toml"));
        assert!(types.contains("imagegen") && types.contains("search"));
        assert_eq!(types.len(), 2);
    }
}
