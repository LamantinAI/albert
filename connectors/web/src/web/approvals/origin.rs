use albert_core::{
    accounts::AccountRole, service::session::web::WebActor, sessions::actor::ActorToolPolicy,
    sessions::UserId,
};
use octo_approval::{Origin, ScopeBinding};
use octo_history::SessionId;

use super::{native::ApprovalOwner, ApprovalService};
use crate::api::{errors::ApiError, errors::ApiResult, principal::Principal};

impl ApprovalService {
    pub(super) async fn owner(&self, caller: &Principal) -> ApiResult<ApprovalOwner> {
        caller.owner_permit()?;
        let grant = self
            .runtime
            .sessions
            .accounts()
            .account(caller.account_id())
            .await
            .map_err(|_| ApiError::forbidden())?;
        if grant.role != AccountRole::Owner
            || Some(grant.user_id.as_str()) != caller.canonical_user_id()
        {
            return Err(ApiError::forbidden());
        }
        self.owner_identity(&grant.account_id, &grant.user_id).await
    }
    pub(super) async fn revalidate_owner(&self, owner: &ApprovalOwner) -> ApiResult<()> {
        let grant = self
            .runtime
            .sessions
            .accounts()
            .account(&owner.account)
            .await
            .map_err(|_| ApiError::forbidden())?;
        if grant.role != AccountRole::Owner || grant.user_id != owner.user {
            return Err(ApiError::forbidden());
        }
        Ok(())
    }
    pub(super) async fn owner_identity(
        &self,
        account: &str,
        user: &str,
    ) -> ApiResult<ApprovalOwner> {
        let user_id = UserId::parse(user).map_err(|_| ApiError::forbidden())?;
        let principal = self
            .runtime
            .sessions
            .web_sql_principal(&user_id)
            .await
            .map_err(|_| ApiError::forbidden())?;
        let destination = format!("user:{user}");
        Ok(ApprovalOwner {
            account: account.into(),
            user: user.into(),
            origin: Origin {
                principal,
                connector: "web".into(),
                conversation: destination.clone(),
                private_destination: destination,
                may_approve: true,
            },
        })
    }
    pub(super) async fn origin_session(
        &self,
        owner: &ApprovalOwner,
        binding: Option<&ScopeBinding>,
    ) -> Option<String> {
        let binding = binding?;
        let raw = if binding.opaque_id.starts_with("account/") {
            let account = self
                .runtime
                .sessions
                .scope_registry()
                .account_for_scope(binding)?;
            if account != owner.account {
                return None;
            }
            binding.epoch.as_str()
        } else if let Some(session) = binding.opaque_id.strip_prefix("session/") {
            session
        } else {
            return None;
        };
        let id = SessionId::parse(raw).ok()?;
        let actor = WebActor {
            account_id: owner.account.clone(),
            user_id: UserId::parse(&owner.user).ok()?,
            tools: ActorToolPolicy::ExistingHost,
        };
        let context = self.runtime.sessions.web_context(&actor, &id).await.ok()?;
        (context.is_private() && context.is_live()).then(|| id.to_string())
    }
    pub(super) async fn require_session(&self, owner: &ApprovalOwner, id: &str) -> ApiResult<()> {
        let id = SessionId::parse(id).map_err(|_| ApiError::not_found())?;
        let actor = WebActor {
            account_id: owner.account.clone(),
            user_id: UserId::parse(&owner.user).map_err(|_| ApiError::forbidden())?,
            tools: ActorToolPolicy::ExistingHost,
        };
        self.runtime
            .sessions
            .web_context(&actor, &id)
            .await
            .map_err(|_| ApiError::not_found())?;
        Ok(())
    }
}
