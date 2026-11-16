use std::{fs::create_dir_all, fs::read, fs::read_dir, fs::write, sync::Arc, time::Duration};

use albert_core::{
    media::encode_prepared, media::input_digest, media::project, media::MediaInput,
    service::session::web::start::StartRecord, service::session::web::WebActor,
    sessions::actor::ActorToolPolicy,
};
use octo_history::{AppendRequest, SessionId, Turn};
use sqlx::query;
use tokio::{spawn, time::sleep, time::timeout};

use crate::web::sessions::{fixture::fixture, send::namespace, SessionHost};

async fn accept(host: &SessionHost, actor: &WebActor, input: &MediaInput) -> StartRecord {
    let request = SessionId::new().to_string();
    host.runtime
        .sessions
        .web_start_create(
            actor,
            &request,
            "first-body",
            false,
            AppendRequest {
                namespace: namespace(actor),
                idempotency_key: request.clone(),
                digest: input.digest(),
                turns: vec![Turn::user(encode_prepared(
                    input.text.clone(),
                    input.assets.clone(),
                    vec![],
                    input.workspace_files.clone(),
                ))],
            },
        )
        .await
        .unwrap()
}

#[tokio::test]
async fn permanent_documents_have_exact_workspace_copies_and_immutable_first_input_recovery() {
    let (_dir, host, caller) = fixture().await;
    let actor = host.actor(&caller).await.unwrap();
    let root = &host.runtime.base.code_workspace;
    create_dir_all(root).unwrap();
    let bytes = vec![b'c'; 2 * 1024 * 1024];
    let store = host.runtime.sessions.media();
    let document = store
        .stage(&actor.account_id, bytes.clone(), "text/csv", "../data.csv")
        .await
        .unwrap();
    let image = store
        .stage(
            &actor.account_id,
            b"\x89PNG\r\n\x1a\nfixture".to_vec(),
            "image/png",
            "image.png",
        )
        .await
        .unwrap();
    let ids = vec![document.id.clone(), image.id.clone()];
    let input = host
        .prepare_media(&actor, None, "Review the data", &ids, false)
        .await
        .unwrap();
    assert_eq!(input.workspace_files.len(), 1);
    let path = input.workspace_files[0].path.as_ref().unwrap();
    assert_eq!(read(root.join(path)).unwrap(), bytes);
    assert_eq!(input.assets[0].filename, "data.csv");
    let row = accept(&host, &actor, &input).await;
    write(root.join(path), b"edited after acceptance").unwrap();
    let recovered = host
        .accepted_media(&actor, &row, &input.text, &ids, &input.digest())
        .await
        .unwrap();
    assert_eq!(recovered.workspace_files[0].path.as_ref(), Some(path));
    assert_eq!(read(root.join(path)).unwrap(), b"edited after acceptance");
    assert_eq!(recovered.blobs[0].bytes().as_ref(), bytes.as_slice());
    let transcript = encode_prepared(input.text, input.assets, vec![], recovered.workspace_files);
    assert!(project(&transcript).contains(path));
    assert!(project(&transcript).len() < 2048);
    assert_eq!(read_dir(root).unwrap().count(), 1);
    assert!(host
        .accepted_media(
            &actor,
            &row,
            "different",
            &[],
            &input_digest("different", &[])
        )
        .await
        .is_err());
}

#[tokio::test]
async fn failed_preparation_is_recorded_and_recovery_does_not_write_or_change_the_outcome() {
    let (_dir, host, caller) = fixture().await;
    let actor = host.actor(&caller).await.unwrap();
    let root = &host.runtime.base.code_workspace;
    assert!(!root.exists());
    let asset = host
        .runtime
        .sessions
        .media()
        .stage(
            &actor.account_id,
            b"%PDF-document".to_vec(),
            "application/pdf",
            "report.pdf",
        )
        .await
        .unwrap();
    let ids = vec![asset.id];
    let input = host
        .prepare_media(&actor, None, "", &ids, false)
        .await
        .unwrap();
    assert_eq!(input.workspace_files.len(), 1);
    assert!(input.workspace_files[0].path.is_none());
    let digest = input.digest();
    let row = accept(&host, &actor, &input).await;
    create_dir_all(root).unwrap();
    let recovered = host
        .accepted_media(&actor, &row, "", &ids, &digest)
        .await
        .unwrap();
    assert!(recovered.workspace_files[0].path.is_none());
    assert_eq!(read_dir(root).unwrap().count(), 0);
    assert!(project(&encode_prepared(
        "".into(),
        recovered.assets,
        vec![],
        recovered.workspace_files
    ))
    .contains("preparation failed"));
}

#[tokio::test]
async fn temporary_and_chat_only_documents_never_materialize_in_the_workspace() {
    let (_dir, host, caller) = fixture().await;
    let actor = host.actor(&caller).await.unwrap();
    let root = &host.runtime.base.code_workspace;
    create_dir_all(root).unwrap();
    let asset = host
        .runtime
        .sessions
        .media()
        .stage(
            &actor.account_id,
            b"PK\x03\x04archive".to_vec(),
            "application/zip",
            "archive.zip",
        )
        .await
        .unwrap();
    let ids = vec![asset.id];
    let input = host
        .prepare_media(&actor, None, "", &ids, true)
        .await
        .unwrap();
    assert!(input.workspace_files.is_empty());
    let temporary = host
        .runtime
        .sessions
        .web_create(&actor, "Temporary", true)
        .await
        .unwrap();
    let input = host
        .prepare_media(&actor, Some(temporary.id.as_str()), "", &ids, false)
        .await
        .unwrap();
    assert!(input.workspace_files.is_empty());
    let restricted = WebActor {
        tools: ActorToolPolicy::ChatOnly,
        ..actor
    };
    let input = host
        .prepare_media(&restricted, None, "", &ids, false)
        .await
        .unwrap();
    assert!(input.workspace_files.is_empty());
    assert_eq!(read_dir(root).unwrap().count(), 0);
}

#[tokio::test]
async fn later_messages_prepare_files_and_a_demoted_owner_cannot_write_new_copies() {
    let (_dir, host, caller) = fixture().await;
    let actor = host.actor(&caller).await.unwrap();
    let root = &host.runtime.base.code_workspace;
    create_dir_all(root).unwrap();
    let session = host
        .runtime
        .sessions
        .web_create(&actor, "Documents", false)
        .await
        .unwrap();
    let asset = host
        .runtime
        .sessions
        .media()
        .stage(
            &actor.account_id,
            b"# Notes".to_vec(),
            "text/markdown",
            "notes.md",
        )
        .await
        .unwrap();
    let ids = vec![asset.id];
    let input = host
        .prepare_media(&actor, Some(session.id.as_str()), "Read", &ids, false)
        .await
        .unwrap();
    assert!(input.workspace_files[0].path.is_some());
    let repeat = host
        .prepare_media(&actor, Some(session.id.as_str()), "Read", &ids, false)
        .await
        .unwrap();
    assert_eq!(
        input.workspace_files[0].path,
        repeat.workspace_files[0].path
    );
    query("UPDATE albert_login_accounts SET role='user' WHERE id=?")
        .bind(&actor.account_id)
        .execute(host.runtime.sessions.database().pool())
        .await
        .unwrap();
    assert!(host
        .prepare_media(&actor, None, "Read", &ids, false)
        .await
        .is_err());
    assert_eq!(read_dir(root).unwrap().count(), 1);
}

#[tokio::test]
async fn first_input_recovery_and_end_use_compatible_lifecycle_lock_order() {
    let (_dir, host, caller) = fixture().await;
    let actor = host.actor(&caller).await.unwrap();
    let asset = host
        .runtime
        .sessions
        .media()
        .stage(
            &actor.account_id,
            b"document".to_vec(),
            "text/plain",
            "report.txt",
        )
        .await
        .unwrap();
    let ids = vec![asset.id];
    let input = host
        .prepare_media(&actor, None, "", &ids, false)
        .await
        .unwrap();
    let digest = input.digest();
    let row = Arc::new(accept(&host, &actor, &input).await);
    let context = host.resource(&actor, &row.session).await.unwrap();
    // Queue recovery at the backend, then queue End at the lifecycle boundary.
    // A reversed backend -> lifecycle order would form a lock cycle on release.
    let backend = host
        .runtime
        .sessions
        .journal_router()
        .handle(context.session_id())
        .await;
    let held = backend.write_owned().await;
    let copy = host.clone();
    let owner = host.actor(&caller).await.unwrap();
    let accepted = row.clone();
    let mut recovery = spawn(async move {
        copy.accepted_media(&owner, &accepted, "", &ids, &digest)
            .await
    });
    sleep(Duration::from_millis(30)).await;
    let copy = host.clone();
    let mut ending = spawn(async move {
        copy.runtime
            .sessions
            .web_end(&actor, context.session_id(), context.session().revision)
            .await
    });
    sleep(Duration::from_millis(30)).await;
    drop(held);
    let result = timeout(Duration::from_secs(3), async {
        let _ = (&mut recovery).await.unwrap();
        (&mut ending).await.unwrap().unwrap();
    })
    .await;
    if result.is_err() {
        recovery.abort();
        ending.abort();
    }
    assert!(result.is_ok(), "Recovery and End must not deadlock");
}
