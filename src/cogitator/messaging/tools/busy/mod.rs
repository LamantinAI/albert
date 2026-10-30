use chrono::Utc;
use octo_history::SessionId;
use serde_json::{json, Value};
use sqlx::query_scalar;

use super::SessionMessagingTool;
use crate::{
    cogitator::AlbertCogitator,
    messaging::contract::{DestinationSpec, MessageDraft},
};

impl SessionMessagingTool {
    /// A one-shot observation, not a dispatch/resolution or a polling loop.
    pub(super) async fn observe_admitted_recipient(
        &self,
        host: &AlbertCogitator,
        draft: &MessageDraft,
    ) -> Value {
        let observed_at = Utc::now().to_rfc3339();
        let Some(service) = &host.session_service else {
            return json!({"state":"unknown","observed_at":observed_at});
        };
        let (candidate, selection) = match &draft.destination {
            DestinationSpec::FixedSession { session, .. } => (Some(session.clone()), "fixed"),
            DestinationSpec::ChannelActive { route } => {
                let actor = self
                    .source
                    .context()
                    .private_user_id()
                    .ok()
                    .map(|user| user.as_str());
                let value = if let Some(actor) = actor {
                    query_scalar::<_,String>("SELECT session_id FROM albert_private_session_routes WHERE connector_id=? AND channel_id=? AND user_id=?")
                        .bind(&route.connector).bind(&route.channel).bind(actor).fetch_optional(service.database().pool()).await.ok().flatten()
                } else {
                    None
                };
                (
                    value.and_then(|value| SessionId::parse(&value).ok()),
                    "channel_active_candidate_not_pinned",
                )
            }
        };
        let Some(candidate) = candidate else {
            return json!({"state":"unknown","selection":selection,"observed_at":observed_at});
        };
        if service
            .get(self.source.context(), &candidate)
            .await
            .is_err()
        {
            return json!({"state":"unavailable","selection":selection,"observed_at":observed_at});
        }
        let state = {
            let Ok(states) = host.turns.try_lock() else {
                return json!({"state":"transition_unknown","selection":selection,"observed_at":observed_at});
            };
            states
                .get(&("session".into(), format!("session/{candidate}")))
                .cloned()
        };
        let activity = if let Some(state) = state {
            match state.try_lock() {
                Ok(gate) if gate.native_active() => "busy_native",
                Ok(gate)
                    if gate
                        .message
                        .as_ref()
                        .is_some_and(|task| !task.is_finished()) =>
                {
                    "busy_message"
                }
                Ok(_) => "idle",
                Err(_) => "transition_unknown",
            }
        } else {
            "idle"
        };
        // A fast recipient may have advanced already. Report a recorded snapshot
        // rather than claiming every accepted request is still not started.
        let work = self
            .runtime
            .store
            .status(&self.source, &draft.id)
            .await
            .ok()
            .and_then(|status| status.work);
        json!({"state":activity,"session_id":candidate,"selection":selection,"work_state":work,"observed_at":observed_at})
    }
}

#[cfg(test)]
mod tests {
    use serde_json::{from_value, json};

    use crate::{
        cogitator::messaging::fixture::Fixture,
        messaging::{contract::MessageId, store::WorkState},
    };

    #[tokio::test]
    async fn explicit_question_to_busy_native_recipient_is_accepted_with_idle_boundary_note() {
        let fixture = Fixture::new("never-busy").await;
        fixture.native(&fixture.b, "hold current work").await;
        fixture.wait_requests(1).await;
        let accepted=fixture.action(json!({"action":"send","destination":{"kind":"fixed_session","session":fixture.b.session_id()},"intent":"question","content":"Please answer when current work is done"})).await;
        assert_eq!(accepted["accepted"], true);
        assert_eq!(accepted["recipient_observation"]["state"], "busy_native");
        assert_eq!(accepted["recipient_observation"]["work_state"], "queued");
        assert!(accepted["queue_note"]
            .as_str()
            .unwrap()
            .contains("idle Session boundary"));
        assert!(accepted["queue_note"]
            .as_str()
            .unwrap()
            .contains("passive inspect/status"));
        let id: MessageId = from_value(accepted["message_id"].clone()).unwrap();
        assert_eq!(fixture.status(&id).await.work, Some(WorkState::Queued));
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.host.stop_turns(&fixture.ctx).await;
    }
    #[tokio::test]
    async fn channel_active_observation_is_only_a_candidate_and_does_not_pin_recipient() {
        let fixture = Fixture::new("healthy").await;
        let accepted=fixture.action(json!({"action":"send","destination":{"kind":"channel_active","route":{"connector":"telegram","channel":"7"}},"intent":"question","content":"Ask the active Session at dispatch"})).await;
        assert_eq!(
            accepted["recipient_observation"]["session_id"],
            fixture.b.session_id().as_str()
        );
        assert_eq!(
            accepted["recipient_observation"]["selection"],
            "channel_active_candidate_not_pinned"
        );
        let id: MessageId = from_value(accepted["message_id"].clone()).unwrap();
        assert!(fixture.status(&id).await.resolved.is_none());
        let c = fixture
            .service
            .create_private(&fixture.b, "Selected before dispatch")
            .await
            .unwrap();
        fixture.host.messaging_tick(&fixture.ctx).await;
        assert_eq!(
            fixture.wait_terminal(&id).await.work,
            Some(WorkState::Completed)
        );
        assert_eq!(
            fixture.status(&id).await.resolved.unwrap().session,
            c.session_id().clone()
        );
        assert_eq!(fixture.server.requests.lock().unwrap().len(), 1);
        fixture.host.stop_turns(&fixture.ctx).await;
    }
}
