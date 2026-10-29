//! Validated skill packages on disk. The host activates the new catalog afterward.
mod files;
mod import;

use std::{
    fs::{read_dir, remove_dir_all, rename},
    sync::Arc,
};

use tokio::sync::Mutex;

use self::files::{
    check_replacement_size, command_available, directory, inspect, package, valid_id,
};
use crate::{
    api::{
        errors::{ApiError, ApiResult},
        host::{HostResponse, SkillOperation},
        models::{EmptyObject, SkillList},
        principal::OwnerPermit,
    },
    config::Config,
    service::panel_models::files::{
        atomic_write_modes, check_revision, checked, ensure_directory_mode, serialized,
    },
    skills::control::inspect_document,
};

#[derive(Clone)]
pub(crate) struct SkillService {
    base: Config,
    write_gate: Arc<Mutex<()>>,
}

impl SkillService {
    pub fn new(base: Config, write_gate: Arc<Mutex<()>>) -> Self {
        Self { base, write_gate }
    }

    pub async fn operation(
        &self,
        _permit: &OwnerPermit,
        operation: SkillOperation,
    ) -> ApiResult<HostResponse> {
        let service = self.clone();
        serialized(Arc::clone(&self.write_gate), move || {
            service.perform(operation)
        })
        .await
    }

    fn perform(&self, operation: SkillOperation) -> ApiResult<HostResponse> {
        let root = directory(
            &self.base,
            !matches!(operation, SkillOperation::List | SkillOperation::Get { .. }),
        )?;
        match operation {
            SkillOperation::List => {
                if !root.exists() {
                    return Ok(HostResponse::Skills(SkillList { items: Vec::new() }));
                }
                let mut items = Vec::new();
                for entry in read_dir(&root).map_err(|_| ApiError::internal())? {
                    let entry = entry.map_err(|_| ApiError::internal())?;
                    let Some(id) = entry.file_name().to_str().map(str::to_owned) else {
                        continue;
                    };
                    if valid_id(&id).is_err() {
                        continue;
                    }
                    if let Some(skill) = package(&root, &id)? {
                        items.push(skill.view(false)?);
                    }
                }
                items.sort_by(|left, right| left.id.cmp(&right.id));
                Ok(HostResponse::Skills(SkillList { items }))
            }
            SkillOperation::Get { id } => Ok(HostResponse::Skill(
                package(&root, &id)?
                    .ok_or_else(ApiError::not_found)?
                    .view(true)?,
            )),
            SkillOperation::Put { id, input } => {
                valid_id(&id)?;
                if input.content.len() > 1024 * 1024 {
                    return Err(ApiError::invalid("Skill instructions exceed 1 MiB."));
                }
                inspect_document(&input.content, &id).map_err(ApiError::invalid)?;
                let previous = package(&root, &id)?;
                if let Some(previous) = &previous {
                    if input.revision.is_none() {
                        return Err(ApiError::conflict(
                            "Reload the skill before replacing its instructions.",
                        ));
                    }
                    check_revision(input.revision.as_deref(), &previous.revision)?;
                    check_replacement_size(previous, &input.content)?;
                } else if input.revision.is_some() {
                    return Err(ApiError::not_found());
                }
                let folder = checked(&root, &root.join(&id))?;
                if previous.is_none() && folder.exists() {
                    return Err(ApiError::conflict(
                        "This directory is not an installed skill package.",
                    ));
                }
                let enabled = previous.as_ref().is_none_or(|previous| previous.enabled);
                command_available(&root, &id, &input.content, enabled)?;
                ensure_directory_mode(&root, &folder, 0o750)?;
                let name = if enabled {
                    "SKILL.md"
                } else {
                    "SKILL.disabled.md"
                };
                atomic_write_modes(
                    &root,
                    &folder.join(name),
                    input.content.as_bytes(),
                    0o640,
                    0o750,
                )?;
                Ok(HostResponse::Skill(
                    package(&root, &id)?
                        .ok_or_else(ApiError::internal)?
                        .view(true)?,
                ))
            }
            SkillOperation::Update { id, input } => {
                let previous = package(&root, &id)?.ok_or_else(ApiError::not_found)?;
                check_revision(Some(&input.revision), &previous.revision)?;
                command_available(&root, &id, &previous.content, input.enabled)?;
                if previous.enabled != input.enabled {
                    let to = previous.folder.join(if input.enabled {
                        "SKILL.md"
                    } else {
                        "SKILL.disabled.md"
                    });
                    if to.exists() {
                        return Err(ApiError::conflict("Skill enable state is ambiguous."));
                    }
                    checked(&root, &to)?;
                    rename(&previous.path, to).map_err(|_| ApiError::internal())?;
                }
                Ok(HostResponse::Skill(
                    package(&root, &id)?
                        .ok_or_else(ApiError::internal)?
                        .view(true)?,
                ))
            }
            SkillOperation::Delete { id, revision } => {
                let previous = package(&root, &id)?.ok_or_else(ApiError::not_found)?;
                if revision.is_none() {
                    return Err(ApiError::conflict("Reload the skill before removing it."));
                }
                check_revision(revision.as_deref(), &previous.revision)?;
                inspect(&previous.folder)?;
                remove_dir_all(previous.folder).map_err(|_| ApiError::internal())?;
                Ok(HostResponse::Empty(EmptyObject {}))
            }
            SkillOperation::Import(input) => self.import(&root, input).map(HostResponse::Skill),
        }
    }
}

#[cfg(test)]
mod tests {
    use std::{
        fs::{hard_link, read, write},
        io::{Cursor, Write},
        os::unix::fs::PermissionsExt,
        sync::Arc,
    };

    use base64::{engine::general_purpose::STANDARD, Engine};
    use rand::random;
    use tempfile::{tempdir, TempDir};
    use tokio::sync::Mutex;
    use zip::{write::SimpleFileOptions, CompressionMethod, ZipWriter};

    use super::SkillService;
    use crate::{
        api::{
            host::{HostResponse, SkillOperation},
            models::{AccountRole, AccountView, SkillImport, SkillUpdate, SkillView, SkillWrite},
            principal::{OwnerPermit, Principal},
        },
        config::Config,
        skills::SkillStore,
    };

    fn permit() -> OwnerPermit {
        Principal::authenticated(
            AccountView {
                id: "owner".into(),
                username: "owner".into(),
                role: AccountRole::Owner,
                canonical_user_id: Some("owner".into()),
                must_change_password: false,
                disabled: false,
                revision: 1,
            },
            "test".into(),
            "csrf".into(),
        )
        .owner_permit()
        .unwrap()
    }

    fn fixture() -> (TempDir, SkillService) {
        let directory = tempdir().unwrap();
        let path = directory.path().join("albert.toml");
        write(
            &path,
            format!("openai_key_env = \"ALBERT_UNUSED_{}\"", random::<u64>()),
        )
        .unwrap();
        let base = Config::load_from(&path).unwrap();
        (directory, SkillService::new(base, Arc::new(Mutex::new(()))))
    }

    fn source(body: &str) -> String {
        format!("---\nname: research\ndescription: Inspect the evidence\ncommand: research\n---\n{body}\n")
    }
    fn skill(response: HostResponse) -> SkillView {
        match response {
            HostResponse::Skill(skill) => skill,
            _ => panic!("Expected a skill response"),
        }
    }
    fn upload(filename: &str, bytes: &[u8], replace: bool) -> SkillOperation {
        SkillOperation::Import(SkillImport {
            filename: filename.into(),
            data_base64: STANDARD.encode(bytes),
            replace,
        })
    }
    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut archive = ZipWriter::new(Cursor::new(Vec::new()));
        for (path, content) in entries {
            archive
                .start_file(
                    *path,
                    SimpleFileOptions::default()
                        .compression_method(CompressionMethod::Deflated)
                        .unix_permissions(0o644),
                )
                .unwrap();
            archive.write_all(content).unwrap();
        }
        archive.finish().unwrap().into_inner()
    }

    #[tokio::test]
    async fn edits_and_enable_flags_use_cas_and_explicit_runtime_refresh() {
        let (directory, service) = fixture();
        let first = skill(
            service
                .operation(
                    &permit(),
                    SkillOperation::Put {
                        id: "research".into(),
                        input: SkillWrite {
                            content: source("Original instructions"),
                            revision: None,
                        },
                    },
                )
                .await
                .unwrap(),
        );
        let store = SkillStore::load(directory.path().join("skills"), 5, 10, &[]);
        assert!(store.catalog().contains("research"));
        let disabled = skill(
            service
                .operation(
                    &permit(),
                    SkillOperation::Update {
                        id: "research".into(),
                        input: SkillUpdate {
                            revision: first.revision.clone(),
                            enabled: false,
                        },
                    },
                )
                .await
                .unwrap(),
        );
        assert!(!disabled.enabled);
        assert!(!directory.path().join("skills/research/SKILL.md").exists());
        let path = directory.path().join("skills/research/SKILL.disabled.md");
        assert!(path.is_file());
        assert_eq!(path.metadata().unwrap().permissions().mode() & 0o777, 0o640);
        assert_eq!(
            path.parent()
                .unwrap()
                .metadata()
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o750
        );
        assert!(service
            .operation(
                &permit(),
                SkillOperation::Put {
                    id: "research".into(),
                    input: SkillWrite {
                        content: source("Must not replace"),
                        revision: Some(first.revision)
                    }
                }
            )
            .await
            .is_err());
        assert!(String::from_utf8(read(&path).unwrap())
            .unwrap()
            .contains("Original instructions"));
        // File changes do not pretend to activate a different running catalog.
        assert!(store.catalog().contains("research"));
        store.refresh(directory.path().join("skills"), &[]);
        assert!(!store.catalog().contains("research"));
    }

    #[tokio::test]
    async fn zip_resources_replace_atomically_and_duplicates_require_confirmation() {
        let (directory, service) = fixture();
        let original = archive(&[
            ("research/SKILL.md", source("First version").as_bytes()),
            ("research/references/old.md", b"old resource"),
        ]);
        let imported = skill(
            service
                .operation(&permit(), upload("research.zip", &original, false))
                .await
                .unwrap(),
        );
        assert_eq!(imported.id, "research");
        let replacement = archive(&[
            ("research/SKILL.md", source("Second version").as_bytes()),
            ("research/scripts/task.py", b"print('done')"),
        ]);
        assert!(service
            .operation(&permit(), upload("research.zip", &replacement, false))
            .await
            .is_err());
        assert!(directory
            .path()
            .join("skills/research/references/old.md")
            .exists());
        let replaced = skill(
            service
                .operation(&permit(), upload("research.zip", &replacement, true))
                .await
                .unwrap(),
        );
        assert!(replaced.content.unwrap().contains("Second version"));
        assert!(!directory
            .path()
            .join("skills/research/references/old.md")
            .exists());
        assert_eq!(
            read(directory.path().join("skills/research/scripts/task.py")).unwrap(),
            b"print('done')"
        );
    }

    #[tokio::test]
    async fn traversal_links_and_zip_bombs_are_rejected_before_installation() {
        let (directory, service) = fixture();
        let outside = directory.path().join("escape");
        let bad = archive(&[
            ("../escape", b"must not escape"),
            ("research/SKILL.md", source("Safe").as_bytes()),
        ]);
        assert!(service
            .operation(&permit(), upload("bad.zip", &bad, false))
            .await
            .is_err());
        assert!(!outside.exists());
        assert!(!directory.path().join("skills/research").exists());
        let mut linked = ZipWriter::new(Cursor::new(Vec::new()));
        linked
            .start_file("research/SKILL.md", SimpleFileOptions::default())
            .unwrap();
        linked.write_all(source("Safe").as_bytes()).unwrap();
        linked
            .add_symlink("research/link", "/etc/passwd", SimpleFileOptions::default())
            .unwrap();
        let linked = linked.finish().unwrap().into_inner();
        assert!(service
            .operation(&permit(), upload("linked.zip", &linked, false))
            .await
            .is_err());
        let bomb = archive(&[
            ("research/SKILL.md", source("Safe").as_bytes()),
            ("research/big.bin", &vec![0; 10 * 1024 * 1024 + 1]),
        ]);
        assert!(service
            .operation(&permit(), upload("bomb.zip", &bomb, false))
            .await
            .is_err());
        assert!(!directory.path().join("skills/research").exists());
    }

    #[tokio::test]
    async fn oversized_instructions_cannot_damage_an_existing_package() {
        let (directory, service) = fixture();
        let original = source("Keep these instructions");
        let installed = skill(
            service
                .operation(&permit(), upload("research.md", original.as_bytes(), false))
                .await
                .unwrap(),
        );
        let oversized = source(&"x".repeat(1024 * 1024));
        let zipped = archive(&[("research/SKILL.md", oversized.as_bytes())]);
        for operation in [
            SkillOperation::Put {
                id: "research".into(),
                input: SkillWrite {
                    content: oversized.clone(),
                    revision: Some(installed.revision.clone()),
                },
            },
            upload("research.md", oversized.as_bytes(), true),
            upload("research.zip", &zipped, true),
        ] {
            assert!(service.operation(&permit(), operation).await.is_err());
            assert_eq!(
                read(directory.path().join("skills/research/SKILL.md")).unwrap(),
                original.as_bytes()
            );
        }
    }

    #[tokio::test]
    async fn duplicate_command_validation_does_not_leave_an_unusable_directory() {
        let (directory, service) = fixture();
        service
            .operation(
                &permit(),
                upload("research.md", source("Installed").as_bytes(), false),
            )
            .await
            .unwrap();
        assert!(service
            .operation(
                &permit(),
                SkillOperation::Put {
                    id: "second".into(),
                    input: SkillWrite {
                        content: source("Conflicting command"),
                        revision: None
                    }
                }
            )
            .await
            .is_err());
        assert!(!directory.path().join("skills/second").exists());
        service.operation(&permit(), SkillOperation::Put { id: "second".into(), input: SkillWrite { content: "---\nname: second\ndescription: Separate skill\ncommand: second\n---\nValid instructions".into(), revision: None } }).await.unwrap();
        assert!(directory.path().join("skills/second/SKILL.md").is_file());
    }

    #[tokio::test]
    async fn instruction_edits_respect_the_preserved_package_size() {
        let (directory, service) = fixture();
        let original = source("Keep this version");
        let installed = skill(
            service
                .operation(&permit(), upload("research.md", original.as_bytes(), false))
                .await
                .unwrap(),
        );
        write(
            directory.path().join("skills/research/resource.bin"),
            vec![0; 10 * 1024 * 1024 - 1024],
        )
        .unwrap();
        assert!(service
            .operation(
                &permit(),
                SkillOperation::Put {
                    id: "research".into(),
                    input: SkillWrite {
                        content: source(&"x".repeat(2048)),
                        revision: Some(installed.revision)
                    }
                }
            )
            .await
            .is_err());
        assert_eq!(
            read(directory.path().join("skills/research/SKILL.md")).unwrap(),
            original.as_bytes()
        );
    }

    #[tokio::test]
    async fn markdown_uploads_support_trusted_external_roots_and_refuse_hard_links() {
        let (_directory, mut service) = fixture();
        let external = tempdir().unwrap();
        service.base.skills_dir = external.path().join("packages");
        let imported = skill(
            service
                .operation(
                    &permit(),
                    upload("SKILL.md", source("Safe markdown").as_bytes(), false),
                )
                .await
                .unwrap(),
        );
        assert_eq!(imported.id, "research");
        let shared = external.path().join("shared");
        write(&shared, "unchanged").unwrap();
        hard_link(&shared, external.path().join("packages/research/hard-link")).unwrap();
        assert!(service
            .operation(
                &permit(),
                SkillOperation::Get {
                    id: "research".into()
                }
            )
            .await
            .is_err());
        assert!(service
            .operation(
                &permit(),
                SkillOperation::Delete {
                    id: "research".into(),
                    revision: None,
                }
            )
            .await
            .is_err());
        assert_eq!(read(shared).unwrap(), b"unchanged");
    }
}
