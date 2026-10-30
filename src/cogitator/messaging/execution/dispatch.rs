use std::{
    convert::Infallible,
    sync::{
        atomic::{AtomicU32, AtomicUsize, Ordering},
        Arc,
    },
    time::Duration,
};

use octo_core::{CogitatorContext, ConnectorId, EventKind};
use octo_rig::{DispatchArgs, OctoDispatchTool};
use rig::{completion::ToolDefinition, tool::Tool};
use serde_json::{json, to_string, Value};

use super::super::tools::authority::ordinary_kind;
use crate::{
    messaging::contract::{DelegationCeiling, ExecutionLocation, Machine},
    sessions::SessionContext,
};

pub(super) struct MessageDispatch {
    pub ctx: CogitatorContext,
    pub source: ConnectorId,
    pub session: SessionContext,
    pub scope: String,
    pub ceiling: DelegationCeiling,
    pub calls: Arc<AtomicU32>,
    pub bytes: Arc<AtomicUsize>,
}
impl MessageDispatch {
    fn inner(&self, target: Option<&str>) -> OctoDispatchTool {
        let catalog = self
            .ctx
            .connectors()
            .iter()
            .filter(|connector| {
                self.ceiling
                    .grants
                    .iter()
                    .any(|grant| grant.connector == connector.id.as_str())
            })
            .map(|connector| {
                format!(
                    "{}: {}",
                    connector.id,
                    connector.capabilities.description.as_deref().unwrap_or("")
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let inner = OctoDispatchTool::new(self.ctx.bus(), self.source.clone(), catalog)
            .with_timeout(Duration::from_secs(360))
            .with_scope(&self.scope)
            .with_context(self.session.clone());
        match target {
            Some(target) => inner.with_context(ExecutionLocation {
                machine: Machine::Host,
                connector: Some(target.into()),
            }),
            None => inner,
        }
    }
}
impl Tool for MessageDispatch {
    const NAME: &'static str = "dispatch_to_connector";
    type Error = Infallible;
    type Args = DispatchArgs;
    type Output = Value;
    async fn definition(&self, prompt: String) -> ToolDefinition {
        self.inner(None).definition(prompt).await
    }
    async fn call(&self, args: DispatchArgs) -> Result<Value, Infallible> {
        if !self.session.is_live() || args.channel.is_some() || !ordinary_kind(&args.kind) {
            return Ok(
                json!({"status":"not_sent","executed":false,"error":"No channel, admin, SQL or expired-scope authority is delegated."}),
            );
        }
        let allowed = self
            .ceiling
            .grants
            .iter()
            .any(|grant| grant.connector == args.target && grant.kinds.contains(&args.kind));
        let current = self.ctx.connectors().iter().any(|connector| {
            connector.id.as_str() == args.target
                && connector.capabilities.description.is_some()
                && connector
                    .capabilities
                    .event_kinds_accept
                    .iter()
                    .any(|pattern| EventKind::new(&args.kind).matches(pattern.as_str()))
        });
        if !allowed || !current {
            return Ok(
                json!({"status":"not_sent","executed":false,"error":"Connector/kind is outside the admitted ceiling."}),
            );
        }
        if self
            .calls
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                left.checked_sub(1)
            })
            .is_err()
        {
            return Ok(
                json!({"status":"not_sent","executed":false,"error":"Tool-call ceiling exhausted."}),
            );
        }
        let target = args.target.clone();
        let result = self.inner(Some(&target)).call(args).await?;
        let encoded = to_string(&result).unwrap_or_default();
        let available = self
            .bytes
            .fetch_update(Ordering::SeqCst, Ordering::SeqCst, |left| {
                Some(left.saturating_sub(encoded.len()))
            })
            .unwrap_or(0);
        if encoded.len() <= available {
            return Ok(result);
        }
        let mut end = available.min(encoded.len());
        while !encoded.is_char_boundary(end) {
            end -= 1;
        }
        Ok(json!({"truncated":true,"output_budget_exhausted":true,"content":&encoded[..end]}))
    }
}
