use std::{
    collections::BTreeMap,
    env::var_os,
    fs::{copy, create_dir_all, read, read_dir, remove_dir_all},
    path::{Path, PathBuf},
};

use serde_json::{from_slice, Value};
use sha2::{Digest, Sha256};

fn files(directory: &Path) -> Vec<PathBuf> {
    let mut result = Vec::new();
    for entry in read_dir(directory).expect("frontend build inputs") {
        let path = entry.expect("read frontend build input").path();
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
    println!("cargo:rerun-if-env-changed=CARGO_FEATURE_EMBEDDED_PANEL");
    if var_os("CARGO_FEATURE_EMBEDDED_PANEL").is_none() {
        return;
    }
    let root = PathBuf::from(var_os("CARGO_MANIFEST_DIR").unwrap()).join("../..");
    let frontend = root.join("web/albert-panel");
    let assets = root.join("web/ui-dist");
    for directory in [
        frontend.join("src"),
        frontend.join("public"),
        frontend.join("scripts"),
        root.join("connectors/web/swagger"),
    ] {
        println!("cargo:rerun-if-changed={}", directory.display());
    }
    let manifest: Value = from_slice(&read(assets.join("manifest.json")).expect(
        "Run npm --prefix web/albert-panel ci && npm --prefix web/albert-panel run build:embedded before Cargo",
    ))
    .expect("embedded frontend manifest");
    let mut inputs = files(&frontend.join("src"));
    inputs.extend(files(&frontend.join("public")));
    inputs.extend(files(&frontend.join("scripts")));
    for name in [
        "index.html",
        "package.json",
        "package-lock.json",
        "tsconfig.json",
        "vite.config.ts",
    ] {
        inputs.push(frontend.join(name));
    }
    inputs.extend(files(&root.join("connectors/web/swagger")));
    inputs.sort();
    let mut digest = Sha256::new();
    for path in inputs {
        println!("cargo:rerun-if-changed={}", path.display());
        digest.update(
            path.strip_prefix(&root)
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
        "Embedded frontend is stale. Run npm --prefix web/albert-panel run build:embedded"
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
    println!("cargo:rerun-if-changed={}", assets.display());
    let output = PathBuf::from(var_os("OUT_DIR").unwrap()).join("panel");
    if output.exists() {
        remove_dir_all(&output).expect("clear embedded asset output");
    }
    for path in files(&assets) {
        let target = output.join(path.strip_prefix(&assets).unwrap());
        create_dir_all(target.parent().unwrap()).expect("embedded asset directory");
        copy(path, target).expect("copy verified embedded asset");
    }
}
