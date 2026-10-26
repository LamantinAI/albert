use octo_connector_telegram::{CleanupRequest, IngressDecision, NativeFrame};
use octo_core::{ConnectorId, Envelope, EventKind};
use octo_history::{PageRequest, SessionId};
use serde_json::json;

use super::{output_scope, policy::SessionTelegramPolicy};
use crate::sessions::{
    error::{SessionError, SessionResult},
    SessionContext, SessionEnded,
};

pub(super) async fn control(
    policy: &SessionTelegramPolicy,
    frame: &NativeFrame,
    mut context: SessionContext,
    name: &str,
    args: &str,
) -> SessionResult<IngressDecision> {
    let mut cleanup = None;
    let mut post_events = Vec::new();
    let notice = match name {
        "sessions" => {
            let before_id = if args.is_empty() {
                None
            } else {
                Some(
                    args.parse::<i64>()
                        .ok()
                        .filter(|id| *id > 0)
                        .ok_or(SessionError::InvalidInput("use /sessions [page-cursor]"))?,
                )
            };
            let page = policy
                .service
                .list_private(
                    &context,
                    PageRequest {
                        before_id,
                        ..PageRequest::default()
                    },
                )
                .await?;
            let mut text = String::from("Saved chats (use /open <id>):");
            for session in page.sessions {
                let title = if session.title.is_empty() {
                    "Untitled"
                } else {
                    &session.title
                };
                text.push_str(&format!(
                    "\n{} — {}{}",
                    session.id,
                    title,
                    if session.id == *context.session_id() {
                        " (current)"
                    } else {
                        ""
                    }
                ));
            }
            if let Some(cursor) = page.next_before.filter(|_| page.has_more) {
                text.push_str(&format!("\nNext page: /sessions {cursor}"));
            }
            text
        }
        "new" => {
            context = policy
                .service
                .create_private_native(&context, args, Some(frame))
                .await?;
            format!("New chat opened: {}", context.session_id())
        }
        "open" => {
            let id = SessionId::parse(args)
                .map_err(|_| SessionError::InvalidInput("use /open <session-id>"))?;
            let revision = context
                .selector_revision()
                .ok_or(SessionError::Unavailable)?;
            context = policy
                .service
                .open_private_native(&context, &id, revision, Some(frame))
                .await?;
            format!(
                "Chat opened: {}. Its history will be used for your next message.",
                context.session_id()
            )
        }
        "tmp" => {
            no_args(args)?;
            context = policy
                .service
                .start_temporary(&context, Some(frame))
                .await?;
            "Temporary chat opened. /save keeps it as a separate chat; /end returns to your previous chat. Unsaved messages are lost on restart. Media input is unavailable until you save.".into()
        }
        "save" => {
            no_args(args)?;
            let key = format!(
                "telegram-save/{}/{}/{}",
                frame.bot_id, frame.chat_id, frame.message_id
            );
            context = policy
                .service
                .save_temporary(&context, &key, Some(frame))
                .await?;
            format!("Chat saved: {}. You can continue here or use /end to return to your previous chat.", context.session_id())
        }
        "end" => {
            no_args(args)?;
            let ended = policy.service.end_temporary(&context, Some(frame)).await?;
            if !ended.discarded {
                policy
                    .saved_ended
                    .lock()
                    .map_err(|_| SessionError::Unavailable)?
                    .insert((ended.cleanup_scope.clone(), ended.scope_epoch.clone()));
            }
            if ended.discarded {
                post_events.push(
                    Envelope::new(
                        ConnectorId::new("albert.sessions"),
                        EventKind::new("albert.session.closed"),
                        json!({}),
                    )
                    .with_context(SessionEnded {
                        session_id: ended.session_id,
                    }),
                );
            }
            cleanup = Some(CleanupRequest {
                scope_key: ended.cleanup_scope,
                epoch: ended.scope_epoch,
            });
            context = ended.context;
            if ended.discarded && !ended.ram_discarded {
                "Temporary chat closed. Some in-memory content could not be purged; it is fenced from further use.".into()
            } else if ended.discarded {
                "Temporary chat discarded. Returned to your previous chat.".into()
            } else {
                "Returned to your previous chat. Your saved chat remains available.".into()
            }
        }
        _ => return Err(SessionError::InvalidInput("unknown session command")),
    };
    Ok(IngressDecision::Control {
        notice: Some(notice),
        scope: Some(output_scope(&context, None).scope),
        post_events,
        cleanup,
    })
}

fn no_args(args: &str) -> SessionResult<()> {
    if args.is_empty() {
        Ok(())
    } else {
        Err(SessionError::InvalidInput(
            "this command takes no arguments",
        ))
    }
}
