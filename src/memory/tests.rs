use std::{path::Path, sync::Arc};

use kaeru_core::{cite, list_initiatives, read_node_full, recall_id_by_name, Store};
use kaeru_rig::KaeruMemory;
use rmcp::model::{CallToolResult, Content};
use toml::from_str;

use super::{
    config::{McpTransport, MemoryConfig},
    migration::{embedded, has_initiative, NAME},
    INITIATIVE,
};

#[test]
fn config_defaults_and_transport_validation() {
    let mut http: MemoryConfig =
        from_str("backend = 'mcp'\ntransport = 'http'\nurl = 'http://localhost:9876/mcp'").unwrap();
    http.validate(Path::new("/deploy"), false).unwrap();
    assert!(http.validate(Path::new("/deploy"), true).is_err());
    let mut stdio: MemoryConfig = from_str(
        "backend = 'mcp'\ntransport = 'stdio'\ncommand = './bin/kaeru-mcp'\nargs = ['--stdio']",
    )
    .unwrap();
    stdio.validate(Path::new("/deploy"), false).unwrap();
    assert!(
        matches!(stdio, MemoryConfig::Mcp { transport: McpTransport::Stdio { command, .. }, .. } if Path::new(&command) == Path::new("/deploy/bin/kaeru-mcp"))
    );
    assert!(matches!(MemoryConfig::default(), MemoryConfig::Embedded));
    for text in [
        "backend = 'other'",
        "backend = 'mcp'\ntransport = 'http'",
        "backend = 'embedded'\nurl = 'http://ignored'",
    ] {
        assert!(from_str::<MemoryConfig>(text).is_err(), "{text}");
    }
    for (url, seconds) in [
        ("file:///tmp/memory", 30),
        ("http://u:secret@host/mcp", 30),
        ("http://localhost/mcp", 0),
    ] {
        let mut cfg = MemoryConfig::Mcp {
            timeout_secs: seconds,
            transport: McpTransport::Http {
                url: url.into(),
                token_env: None,
            },
        };
        assert!(cfg.validate(Path::new("."), false).is_err());
    }
}

#[tokio::test]
async fn native_migration_is_idempotent_and_does_not_change_existing_memory() {
    let store = Arc::new(Store::open_in_memory().unwrap());
    let memory = KaeruMemory::with_initiative(store.clone(), INITIATIVE);
    embedded(&memory).await.unwrap();
    let id = store.scoped(Some(INITIATIVE), |s| {
        recall_id_by_name(s, NAME).unwrap().unwrap()
    });
    let before = read_node_full(&store, &id).unwrap().unwrap();
    embedded(&memory).await.unwrap();
    let after = read_node_full(&store, &id).unwrap().unwrap();
    assert_eq!(before.body, after.body);
    assert_eq!(list_initiatives(&store).unwrap(), [INITIATIVE]);

    let existing = Arc::new(Store::open_in_memory().unwrap());
    existing.scoped(Some(INITIATIVE), |s| {
        cite(s, "user-fact", None, "keep me").unwrap()
    });
    embedded(&KaeruMemory::with_initiative(existing.clone(), INITIATIVE))
        .await
        .unwrap();
    assert!(existing
        .scoped(Some(INITIATIVE), |s| recall_id_by_name(s, NAME).unwrap())
        .is_none());
}

#[test]
fn migration_fails_closed_on_unexpected_or_incomplete_responses() {
    let result = |text| CallToolResult::success(vec![Content::text(text)]);
    assert!(
        has_initiative(&result("initiatives (2):\n  - albert-archive\n  - kaeru"))
            .is_ok_and(|v| !v)
    );
    assert!(has_initiative(&result("initiatives (2):\n  - albert\n  - kaeru")).unwrap());
    for text in [
        "database offline",
        "initiatives (2):\n  - kaeru",
        "initiatives (1):\n  albert",
    ] {
        assert!(has_initiative(&result(text)).is_err());
    }
    assert!(has_initiative(&CallToolResult::error(vec![Content::text("offline")])).is_err());
}
