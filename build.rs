use std::{
    collections::BTreeMap,
    env::var_os,
    fs::{read, read_dir},
    path::{Path, PathBuf},
};

use serde_json::{from_slice, Value};
use sha2::{Digest, Sha256};

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for entry in read_dir(directory)
        .expect("frontend build inputs")
        .flatten()
    {
        let path = entry.path();
        assert!(!path.is_symlink(), "Frontend assets cannot be symlinks");
        if path.is_dir() {
            result.extend(files(&path));
        } else {
            result.push(path);
        }
    }
    result.sort();
    result
}

fn main() {
    let root = PathBuf::from(var_os("CARGO_MANIFEST_DIR").unwrap());
    let frontend = root.join("frontend");
    let assets = root.join("ui-dist");
    let manifest: Value = from_slice(&read(assets.join("manifest.json")).expect(
        "Run npm --prefix frontend ci && npm --prefix frontend run build:embedded before Cargo",
    ))
    .expect("embedded frontend manifest");
    let mut inputs = files(&frontend.join("src"));
    inputs.extend(files(&frontend.join("public")));
    for name in [
        "index.html",
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "vite.config.ts",
        "scripts/build-embedded.mjs",
    ] {
        inputs.push(frontend.join(name));
    }
    inputs.sort();
    let mut digest = Sha256::new();
    for path in inputs {
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update(
            path.strip_prefix(&frontend)
                .unwrap()
                .to_string_lossy()
                .replace('\\', "/")
                .as_bytes(),
        );
        digest.update(read(&path).expect("frontend input readable"));
    }
    assert_eq!(
        manifest["source_sha256"].as_str(),
        Some(format!("{:x}", digest.finalize()).as_str()),
        "Embedded frontend is stale. Run npm --prefix frontend run build:embedded"
    );
    let expected = manifest["assets"]
        .as_object()
        .expect("asset fingerprint map");
    let actual: BTreeMap<String, String> = files(&assets)
        .into_iter()
        .filter(|path| path.file_name().unwrap() != "manifest.json")
        .map(|path| {
            (
                path.strip_prefix(&assets)
                    .unwrap()
                    .to_string_lossy()
                    .replace('\\', "/"),
                format!("{:x}", Sha256::digest(read(path).unwrap())),
            )
        })
        .collect();
    assert_eq!(expected.len(), actual.len(), "Unexpected embedded assets");
    for (name, sha) in actual {
        assert_eq!(
            expected.get(&name).and_then(Value::as_str),
            Some(sha.as_str()),
            "Embedded asset checksum: {name}"
        );
    }
    println!("cargo:rerun-if-changed=ui-dist");
}
