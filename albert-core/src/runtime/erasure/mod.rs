use std::fs::symlink_metadata;

use octo_history::SessionId;
use tokio::task::spawn_blocking;
use tracing::warn;

use super::AgentRuntime;
use crate::{artifacts::Artifacts, media::discard_input_file};

impl AgentRuntime {
    /// Finish one durable deletion independently of model availability. Pending
    /// rows survive both HTTP disconnects and process/configuration changes.
    pub async fn finish_session_deletion(&self, id: &SessionId) -> Result<(), String> {
        let _guard = self.erasure.lock().await;
        let Some(files) = self
            .sessions
            .erasure_files(id)
            .await
            .map_err(|_| "Deletion cleanup could not be loaded.")?
        else {
            return Ok(());
        };
        if let Some(agent) = self.current() {
            let context = self.context().ok_or("Runtime cleanup is starting.")?;
            agent.panel_discard_session(id, &context).await?;
        }
        self.sessions.credentials().forget_session(id.as_str());
        self.sessions.confirmations().forget_session(id.as_str());
        let conversation = ("session".to_owned(), format!("session/{id}"));
        spawn_blocking(move || {
            let metadata = symlink_metadata(&files.workspace)
                .map_err(|_| "The original workspace is unavailable for cleanup.")?;
            if !metadata.is_dir() || metadata.is_symlink() {
                return Err("The original workspace is unavailable for cleanup.");
            }
            Artifacts::discard_conversation(&files.workspace, &conversation)
                .map_err(|_| "Conversation artifact cleanup is still pending.")?;
            for (asset, root) in files.assets {
                discard_input_file(&root, &asset)
                    .map_err(|_| "Attachment copy cleanup is still pending.")?;
            }
            Ok::<(), &'static str>(())
        })
        .await
        .map_err(|_| "Conversation cleanup was interrupted.")??;
        self.sessions
            .acknowledge_erasure(id)
            .await
            .map_err(|_| "Cleanup completion could not be recorded.".to_owned())
    }
    pub(super) async fn recover_session_deletions(&self) {
        let pending = match self.sessions.pending_erasures().await {
            Ok(pending) => pending,
            Err(error) => {
                warn!(%error, "could not load pending conversation cleanup");
                return;
            }
        };
        for id in pending {
            if let Err(error) = self.finish_session_deletion(&id).await {
                warn!(%error, "conversation cleanup remains pending");
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use octo_history::SessionId;
    use sqlx::query;
    use std::{
        fs::create_dir_all, fs::read, fs::rename, fs::write, sync::atomic::AtomicBool, sync::Arc,
    };

    use super::AgentRuntime;
    use crate::{
        config::Config, service::session::web::tests::fixture, sessions::transport::LocalTransport,
    };

    #[tokio::test]
    async fn cleanup_recovers_without_a_model_and_keeps_the_original_workspace_after_reconfiguration(
    ) {
        let (dir, service, actor, _) = fixture().await;
        let original = dir.path().join("original");
        let changed = dir.path().join("changed");
        create_dir_all(&original).unwrap();
        create_dir_all(&changed).unwrap();
        let session = service.web_create(&actor, "Files", false).await.unwrap();
        let asset = SessionId::new().to_string();
        query("INSERT INTO albert_media_assets(id,session_id,filename,content_type,size,bytes) VALUES (?,?,'fixture.bin','application/octet-stream',3,?)")
            .bind(&asset).bind(session.id.as_str()).bind(b"abc".as_slice()).execute(service.database().pool()).await.unwrap();
        let name = format!("media-{asset}");
        write(original.join(&name), "abc").unwrap();
        write(original.join("independent.txt"), "keep").unwrap();
        write(changed.join(&name), "other workspace").unwrap();
        service
            .web_delete(&actor, &session.id, session.revision, &original)
            .await
            .unwrap();
        let path = dir.path().join("albert.toml");
        let mut config = Config::parse_text("", &path).unwrap();
        config.code_workspace = changed.clone();
        let authority = config.authorization.build();
        let runtime = AgentRuntime::new(
            path,
            config,
            Arc::new(service),
            authority,
            Arc::new(AtomicBool::new(false)),
            Arc::new(LocalTransport),
        );
        assert!(runtime.current().is_none());
        runtime.recover_session_deletions().await;
        assert!(!original.join(&name).exists());
        assert_eq!(read(original.join("independent.txt")).unwrap(), b"keep");
        assert_eq!(read(changed.join(&name)).unwrap(), b"other workspace");
        assert!(runtime
            .sessions
            .pending_erasures()
            .await
            .unwrap()
            .is_empty());
        runtime.finish_session_deletion(&session.id).await.unwrap();
    }

    #[tokio::test]
    async fn unexpected_directory_keeps_cleanup_retryable_instead_of_erasing_unrelated_files() {
        let (dir, service, actor, _) = fixture().await;
        let root = dir.path().join("workspace");
        create_dir_all(&root).unwrap();
        let session = service
            .web_create(&actor, "Retry cleanup", false)
            .await
            .unwrap();
        let asset = SessionId::new().to_string();
        query("INSERT INTO albert_media_assets(id,session_id,filename,content_type,size,bytes) VALUES (?,?,'fixture.bin','application/octet-stream',3,?)")
            .bind(&asset).bind(session.id.as_str()).bind(b"abc".as_slice()).execute(service.database().pool()).await.unwrap();
        let managed = root.join(format!("media-{asset}"));
        create_dir_all(&managed).unwrap();
        write(managed.join("keep.txt"), "keep").unwrap();
        service
            .web_delete(&actor, &session.id, session.revision, &root)
            .await
            .unwrap();
        let path = dir.path().join("albert.toml");
        let config = Config::parse_text("", &path).unwrap();
        let authority = config.authorization.build();
        let runtime = AgentRuntime::new(
            path,
            config,
            Arc::new(service),
            authority,
            Arc::new(AtomicBool::new(false)),
            Arc::new(LocalTransport),
        );
        assert!(runtime.finish_session_deletion(&session.id).await.is_err());
        assert_eq!(
            runtime.sessions.pending_erasures().await.unwrap(),
            vec![session.id.clone()]
        );
        assert_eq!(read(managed.join("keep.txt")).unwrap(), b"keep");
        rename(&managed, root.join("retained-directory")).unwrap();
        runtime.recover_session_deletions().await;
        assert!(runtime
            .sessions
            .pending_erasures()
            .await
            .unwrap()
            .is_empty());
        assert_eq!(
            read(root.join("retained-directory/keep.txt")).unwrap(),
            b"keep"
        );
    }
}
