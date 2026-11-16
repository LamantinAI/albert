use std::collections::HashSet;

use super::{conversation, handle::ApprovalHandle, native::ApprovalOwner, ApprovalService};
use crate::api::{
    errors::ApiError, errors::ApiResult, host::ApprovalOperation, host::HostResponse,
    models::ApprovalList, models::ApprovalView, principal::Principal,
};

impl ApprovalService {
    pub(super) async fn get(
        &self,
        owner: &ApprovalOwner,
        id: &str,
        details: bool,
    ) -> ApiResult<ApprovalView> {
        match ApprovalHandle::decode(id)? {
            ApprovalHandle::Confirmation { request } => self.confirmation(owner, &request, details),
            ApprovalHandle::Native {
                adapter,
                executor,
                operation,
            } => {
                let adapter = self
                    .adapters
                    .iter()
                    .find(|item| item.id() == adapter)
                    .ok_or_else(ApiError::not_found)?;
                let ctx = self.context()?;
                match adapter
                    .get(&ctx, owner, &executor, &operation, details)
                    .await
                {
                    Ok(record) => self.remember(owner, record).await,
                    Err(error) if !details => self
                        .cached(owner, id)
                        .filter(|view| !view.can_decide)
                        .ok_or(error),
                    Err(error) => Err(error),
                }
            }
        }
    }
    pub(super) async fn list(
        &self,
        owner: &ApprovalOwner,
        session: Option<&str>,
    ) -> ApiResult<ApprovalList> {
        if let Some(session) = session {
            self.require_session(owner, session).await?;
        }
        let mut items: Vec<_> = self
            .runtime
            .sessions
            .confirmations()
            .list(&owner.account, &owner.user, session)
            .into_iter()
            .map(|value| conversation::view(value, false))
            .collect();
        let mut warnings = Vec::new();
        let context = self.context();
        for adapter in &self.adapters {
            let result = match &context {
                Ok(ctx) => adapter.list(ctx, owner).await,
                Err(_) => Err(ApiError::unavailable("Transport is starting.")),
            };
            match result {
                Ok(records) => {
                    for record in records {
                        match self.remember(owner,record).await {Ok(view)=>items.push(view),Err(_)=>warnings.push("Some protected actions could not be loaded. Check their status before continuing.".into())}
                    }
                }
                Err(_) => warnings.push(
                    "Some protected actions are unavailable. Retry loading their status.".into(),
                ),
            }
        }
        // Native pending-list absence never resolves a dispatched or unknown action.
        for id in self.unresolved(owner) {
            if let Ok(view) = self.get(owner, &id, false).await {
                items.push(view);
            }
        }
        let mut seen = HashSet::new();
        items.retain(|view| {
            session.is_none_or(|id| view.session_id.as_deref() == Some(id))
                && seen.insert(view.id.clone())
        });
        warnings.sort();
        warnings.dedup();
        Ok(ApprovalList { items, warnings })
    }
    pub(super) async fn decide(
        &self,
        owner: &ApprovalOwner,
        id: &str,
        approve: bool,
    ) -> ApiResult<ApprovalView> {
        match ApprovalHandle::decode(id)? {
            ApprovalHandle::Confirmation { request } => {
                self.decide_confirmation(owner, &request, approve).await
            }
            ApprovalHandle::Native {
                adapter,
                executor: _,
                operation: _,
            } => {
                let adapter = self
                    .adapters
                    .iter()
                    .find(|item| item.id() == adapter)
                    .ok_or_else(ApiError::not_found)?;
                let current = self.get(owner, id, false).await?;
                let context = self.context()?;
                self.revalidate_owner(owner).await?;
                let Some(record) = self.begin_decision(owner, &current.id)? else {
                    return self
                        .cached(owner, &current.id)
                        .ok_or_else(ApiError::not_found);
                };
                match adapter.decide(&context, owner, &record, approve).await {
                    Ok(record) => self.remember(owner, record).await,
                    Err(_) => self.decision_uncertain(owner, &current.id),
                }
            }
        }
    }
    pub async fn operation(
        &self,
        caller: &Principal,
        operation: ApprovalOperation,
    ) -> ApiResult<HostResponse> {
        let owner = self.owner(caller).await?;
        match operation {
            ApprovalOperation::List => Ok(HostResponse::Approvals(self.list(&owner, None).await?)),
            ApprovalOperation::Filtered(query) => Ok(HostResponse::Approvals(
                self.list(&owner, query.session_id.as_deref()).await?,
            )),
            ApprovalOperation::Get { id, details } => Ok(HostResponse::Approval(
                self.get(&owner, &id, details).await?,
            )),
            ApprovalOperation::Decide { id, input } => Ok(HostResponse::Approval(
                self.decide(&owner, &id, input.approve).await?,
            )),
        }
    }
}
