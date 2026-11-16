//! skills HTTP resources.
use axum::{extract::State, http::HeaderMap, Json};
use utoipa_axum::{router::OpenApiRouter, routes};

use super::{
    actor::OwnerActor, host::run, input::resource, input::revision, input::ApiJson, input::ApiPath,
    input::IgnoredBody,
};
use crate::api::models::{
    ApiErrorBody, EmptyObject, SkillImport, SkillList, SkillUpdate, SkillView, SkillWrite,
};
use crate::api::{errors::ApiResult, host::HostOperation, host::SkillOperation, state::ApiState};

pub(super) fn router() -> OpenApiRouter<ApiState> {
    OpenApiRouter::new()
        .routes(routes!(get_skills))
        .routes(routes!(get_skills_id))
        .routes(routes!(put_skills_id))
        .routes(routes!(patch_skills_id))
        .routes(routes!(delete_skills_id))
        .routes(routes!(post_skill_imports))
}

#[utoipa::path(get, operation_id = "skills_get_skills", path = "/skills", summary = "Get skills", tag = "skills",
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SkillList),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn get_skills(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
) -> ApiResult<Json<SkillList>> {
    let operation = HostOperation::Skill(SkillOperation::List);
    run(&state, &caller, operation).await
}

#[utoipa::path(get, operation_id = "skills_get_skills_id", path = "/skills/{id}", summary = "Get skills", tag = "skills",
    params(("id" = String, Path, description = "Resource identifier")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SkillView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn get_skills_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
) -> ApiResult<Json<SkillView>> {
    let operation = HostOperation::Skill(SkillOperation::Get { id: resource(id)? });
    run(&state, &caller, operation).await
}

#[utoipa::path(put, operation_id = "skills_put_skills_id", path = "/skills/{id}", summary = "Update skills", tag = "skills",
    request_body = SkillWrite,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SkillView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn put_skills_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<SkillWrite>,
) -> ApiResult<Json<SkillView>> {
    let operation = HostOperation::Skill(SkillOperation::Put {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(patch, operation_id = "skills_patch_skills_id", path = "/skills/{id}", summary = "Update skills", tag = "skills",
    request_body = SkillUpdate,
    params(("id" = String, Path, description = "Resource identifier"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SkillView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn patch_skills_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    ApiJson(input): ApiJson<SkillUpdate>,
) -> ApiResult<Json<SkillView>> {
    let operation = HostOperation::Skill(SkillOperation::Update {
        id: resource(id)?,
        input: input,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(delete, operation_id = "skills_delete_skills_id", path = "/skills/{id}", summary = "Remove skills", tag = "skills",
    params(("id" = String, Path, description = "Resource identifier"), ("If-Match" = Option<String>, Header, description = "Exact configuration revision"), ("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = EmptyObject),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn delete_skills_id(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiPath(id): ApiPath<String>,
    headers: HeaderMap,
    _body: IgnoredBody,
) -> ApiResult<Json<EmptyObject>> {
    let operation = HostOperation::Skill(SkillOperation::Delete {
        id: resource(id)?,
        revision: revision(&headers)?,
    });
    run(&state, &caller, operation).await
}

#[utoipa::path(post, operation_id = "skills_post_skill_imports", path = "/skill-imports", summary = "Create skill imports", tag = "skills",
    request_body = SkillImport,
    params(("x-albert-csrf" = String, Header, description = "Current browser CSRF value")),
    security(("browserCookie" = [])),
    responses((status = 200, description = "Success", body = SkillView),
        (status = 400, description = "Invalid request", body = ApiErrorBody),
        (status = 401, description = "Sign-in required", body = ApiErrorBody),
        (status = 403, description = "Insufficient authority", body = ApiErrorBody),
        (status = 404, description = "Resource unavailable", body = ApiErrorBody),
        (status = 409, description = "Conflicting state", body = ApiErrorBody),
        (status = 412, description = "Revision changed", body = ApiErrorBody),
        (status = 413, description = "Body too large", body = ApiErrorBody),
        (status = 503, description = "Service unavailable", body = ApiErrorBody),
        (status = 500, description = "Internal failure", body = ApiErrorBody))
)]
async fn post_skill_imports(
    State(state): State<ApiState>,
    OwnerActor(caller): OwnerActor,
    ApiJson(input): ApiJson<SkillImport>,
) -> ApiResult<Json<SkillView>> {
    let operation = HostOperation::Skill(SkillOperation::Import(input));
    run(&state, &caller, operation).await
}
