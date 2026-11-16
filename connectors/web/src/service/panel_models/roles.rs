use albert_core::models::{PoolConfig, RoleProfile};

use super::{mutations::require_revision, ModelService};
use crate::api::{
    errors::ApiError, errors::ApiResult, host::ModelRoleOperation, models::ModelRoleWrite,
};

impl ModelService {
    pub(super) fn mutate_role(
        pool: &mut PoolConfig,
        operation: ModelRoleOperation,
        revision: &str,
    ) -> ApiResult<()> {
        match operation {
            ModelRoleOperation::Create(input) => {
                require_revision(Some(&input.revision), revision)?;
                if pool.roles.iter().any(|role| role.id == input.id) {
                    return Err(ApiError::conflict("A role with this name already exists."));
                }
                if pool.roles.len() >= 64 {
                    return Err(ApiError::invalid("Keep at most 64 role assignments."));
                }
                validate(&input)?;
                pool.roles.push(RoleProfile {
                    id: input.id,
                    description: input.description,
                    models: vec![input.model_id],
                    reasoning_effort: None,
                });
            }
            ModelRoleOperation::Update { id, input } => {
                require_revision(Some(&input.revision), revision)?;
                validate(&input)?;
                if id != input.id {
                    return Err(ApiError::invalid("The role name must match its resource."));
                }
                let role = pool
                    .roles
                    .iter_mut()
                    .find(|role| role.id == id)
                    .ok_or_else(ApiError::not_found)?;
                // A simple assignment edit changes its first model only. Preserve
                // configured fallback order and effort rather than flattening it.
                role.models = [input.model_id.clone()]
                    .into_iter()
                    .chain(
                        role.models
                            .iter()
                            .skip(1)
                            .filter(|model| **model != input.model_id)
                            .cloned(),
                    )
                    .collect();
                role.description = input.description;
            }
            ModelRoleOperation::Delete {
                id,
                revision: expected,
            } => {
                require_revision(expected.as_deref(), revision)?;
                let index = pool
                    .roles
                    .iter()
                    .position(|role| role.id == id)
                    .ok_or_else(ApiError::not_found)?;
                pool.roles.remove(index);
            }
        }
        Ok(())
    }
}
fn validate(input: &ModelRoleWrite) -> ApiResult<()> {
    if input.id.trim().is_empty() || input.id.len() > 128 || input.description.len() > 16384 {
        return Err(ApiError::invalid(
            "A role needs a name of up to 128 bytes and a description of up to 16384 bytes.",
        ));
    }
    Ok(())
}
