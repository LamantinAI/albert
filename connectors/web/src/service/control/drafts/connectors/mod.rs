//! Regression checks for owner connector projections, independent of factories.
#[cfg(test)]
mod tests {
    use crate::{
        api::models::{ConfigDraftInput, ConfigFieldChange, ConfigWrite},
        service::control::fixture::fixture,
    };
    use serde_json::json;

    #[tokio::test]
    async fn each_base_type_has_fields_and_edits_preserve_unknown_content_and_comments() {
        let (_dir, service, _owner) = fixture().await;
        for (kind, path, value) in [
            ("search", "connector.default_limit", json!(12)),
            ("browser", "connector.headless", json!(false)),
            ("storage", "connector.root", json!("new-objects")),
            ("speak", "connector.voice", json!("ember")),
        ] {
            let id = format!("connector:{kind}");
            let original = service.document(&id).unwrap();
            let content=format!("# keep comment\n[connector]\nid='{kind}'\ntype='{kind}'\nroot='objects'\n[future]\nkeep='unchanged'\n");
            let saved = service
                .put(
                    &id,
                    ConfigWrite {
                        path: id.clone(),
                        content: content.clone(),
                        revision: Some(original.revision),
                    },
                )
                .await
                .unwrap();
            let edited = service
                .draft(
                    &id,
                    ConfigDraftInput {
                        content: content.clone(),
                        revision: saved.revision.clone(),
                        changes: vec![ConfigFieldChange {
                            path: path.into(),
                            value: value.clone(),
                        }],
                    },
                )
                .await
                .unwrap();
            assert!(edited.content.contains("# keep comment"));
            assert!(edited.content.contains("keep='unchanged'"));
            assert_eq!(
                edited
                    .fields
                    .iter()
                    .find(|f| f.path == path)
                    .unwrap()
                    .value
                    .as_ref(),
                Some(&value)
            );
            assert_eq!(
                service.document(&id).unwrap().content,
                content,
                "drafts must not write files"
            );
            let projected = service
                .draft(
                    &id,
                    ConfigDraftInput {
                        content: edited.content.clone(),
                        revision: saved.revision,
                        changes: vec![],
                    },
                )
                .await
                .unwrap();
            assert_eq!(edited.content, projected.content);
        }
    }

    #[tokio::test]
    async fn projection_rejects_identity_type_unknown_fields_required_reset_and_stale_revision() {
        let (_dir, service, _owner) = fixture().await;
        let id = "connector:storage";
        let old = service.document(id).unwrap();
        let source = "[connector]\nid='storage'\ntype='storage'\nroot='objects'\n";
        let saved = service
            .put(
                id,
                ConfigWrite {
                    path: id.into(),
                    content: source.into(),
                    revision: Some(old.revision),
                },
            )
            .await
            .unwrap();
        for (content, revision, changes) in [
            (
                source.replace("id='storage'", "id='different'"),
                saved.revision.clone(),
                vec![],
            ),
            (
                source.replace("type='storage'", "type='browser'"),
                saved.revision.clone(),
                vec![],
            ),
            (source.into(), "stale".into(), vec![]),
            (
                source.into(),
                saved.revision.clone(),
                vec![ConfigFieldChange {
                    path: "connector.root".into(),
                    value: json!(null),
                }],
            ),
            (
                source.into(),
                saved.revision.clone(),
                vec![ConfigFieldChange {
                    path: "connector.owner_chat".into(),
                    value: json!(7),
                }],
            ),
        ] {
            assert!(service
                .draft(
                    id,
                    ConfigDraftInput {
                        content,
                        revision,
                        changes
                    }
                )
                .await
                .is_err());
        }
        assert_eq!(service.document(id).unwrap().content, source);
    }

    #[tokio::test]
    async fn lists_numbers_and_choices_are_typed_and_safe_without_default_insertion() {
        use super::super::{connector_fields, transform_fields};
        for (kind, changes) in [
            (
                "telegram",
                vec![ConfigFieldChange {
                    path: "connector.address_names".into(),
                    value: json!(["Albert", "Helper"]),
                }],
            ),
            (
                "transcribe",
                vec![ConfigFieldChange {
                    path: "connector.chunk_secs".into(),
                    value: json!(240.5),
                }],
            ),
        ] {
            let content = format!("[connector]\nid='{kind}'\ntype='{kind}'\n");
            let result = transform_fields(
                ConfigDraftInput {
                    content,
                    revision: "same".into(),
                    changes,
                },
                connector_fields(kind),
            )
            .unwrap();
            assert!(!result.content.contains("parallel_uploads"));
        }
        let bad = ConfigDraftInput {
            content: "[connector]\nid='speak'\ntype='speak'".into(),
            revision: "same".into(),
            changes: vec![ConfigFieldChange {
                path: "connector.voice".into(),
                value: json!("invented-voice"),
            }],
        };
        assert!(transform_fields(bad, connector_fields("speak")).is_err());
    }

    #[test]
    fn clearing_optional_http_timeout_removes_empty_group_and_keeps_comments() {
        use super::super::{connector_fields, transform_fields};
        let source = "[connector]\nid='http'\ntype='http'\nbase_url='https://example.org'\n[connector.timeout] # timeout explanation\nrequest_ms=1000 # request explanation\n";
        let result = transform_fields(
            ConfigDraftInput {
                content: source.into(),
                revision: "r1".into(),
                changes: vec![ConfigFieldChange {
                    path: "connector.timeout.request_ms".into(),
                    value: json!(null),
                }],
            },
            connector_fields("http"),
        )
        .unwrap();
        assert!(!result.content.contains("[connector.timeout]"));
        assert!(result.content.contains("# timeout explanation"));
        assert!(result.content.contains("# request explanation"));
    }
}
