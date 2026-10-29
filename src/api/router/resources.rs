use axum::http::{header::IF_MATCH, HeaderMap};
use std::collections::HashMap;

use super::{authorization::host_access, id, parse, RouteSpec};
use crate::api::{
    errors::{ApiError, ApiResult},
    host::{
        ApprovalOperation, ConfigOperation, HostOperation, HostResponse, ModelOperation,
        ProfileOperation, SessionOperation, SkillOperation,
    },
    models::PageQuery,
    principal::Principal,
    state::ApiState,
};

pub(super) async fn host_request(
    state: &ApiState,
    caller: &Principal,
    spec: RouteSpec,
    params: &HashMap<String, String>,
    query: &HashMap<String, String>,
    headers: &HeaderMap,
    body: &[u8],
) -> ApiResult<HostResponse> {
    let resource = || id(params, "id");
    let operation = match (spec.method, spec.path) {
        ("GET", "/account/profile") => HostOperation::Profile(ProfileOperation::Get),
        ("GET", "/account/identities") => HostOperation::Profile(ProfileOperation::Identities),
        ("GET", "/sessions") => HostOperation::Session(SessionOperation::List(page(query)?)),
        ("POST", "/sessions") => HostOperation::Session(SessionOperation::Create(parse(body)?)),
        ("GET", "/sessions/{id}") => {
            HostOperation::Session(SessionOperation::Get { id: resource()? })
        }
        ("PATCH", "/sessions/{id}") => HostOperation::Session(SessionOperation::Update {
            id: resource()?,
            input: parse(body)?,
        }),
        ("GET", "/sessions/{id}/messages") => HostOperation::Session(SessionOperation::Messages {
            id: resource()?,
            page: page(query)?,
        }),
        ("POST", "/sessions/{id}/messages") => HostOperation::Session(SessionOperation::Send {
            id: resource()?,
            input: parse(body)?,
        }),
        ("GET", "/sessions/{id}/messages/{request_id}") => {
            HostOperation::Session(SessionOperation::Receipt {
                id: resource()?,
                request_id: id(params, "request_id")?,
            })
        }
        ("GET", "/sessions/{id}/state") => {
            HostOperation::Session(SessionOperation::State { id: resource()? })
        }
        ("POST", "/attempts/{id}/cancellations") => {
            HostOperation::Session(SessionOperation::Cancel {
                attempt_id: resource()?,
                input: parse(body)?,
            })
        }
        ("GET", "/model-pool") => HostOperation::Model(ModelOperation::List),
        ("POST", "/models") => HostOperation::Model(ModelOperation::Create(parse(body)?)),
        ("PUT", "/models/{id}") => HostOperation::Model(ModelOperation::Update {
            id: resource()?,
            input: parse(body)?,
        }),
        ("DELETE", "/models/{id}") => HostOperation::Model(ModelOperation::Delete {
            id: resource()?,
            revision: revision(headers)?,
        }),
        ("PUT", "/model-pool/order") => HostOperation::Model(ModelOperation::Order(parse(body)?)),
        ("PUT", "/model-pool/preference") => {
            HostOperation::Model(ModelOperation::Preference(parse(body)?))
        }
        ("GET", "/skills") => HostOperation::Skill(SkillOperation::List),
        ("GET", "/skills/{id}") => HostOperation::Skill(SkillOperation::Get { id: resource()? }),
        ("PUT", "/skills/{id}") => HostOperation::Skill(SkillOperation::Put {
            id: resource()?,
            input: parse(body)?,
        }),
        ("PATCH", "/skills/{id}") => HostOperation::Skill(SkillOperation::Update {
            id: resource()?,
            input: parse(body)?,
        }),
        ("DELETE", "/skills/{id}") => HostOperation::Skill(SkillOperation::Delete {
            id: resource()?,
            revision: revision(headers)?,
        }),
        ("POST", "/skill-imports") => HostOperation::Skill(SkillOperation::Import(parse(body)?)),
        ("GET", "/configuration") => HostOperation::Config(ConfigOperation::List),
        ("GET", "/configuration/{id}") => {
            HostOperation::Config(ConfigOperation::Get { id: resource()? })
        }
        ("PUT", "/configuration/{id}") => HostOperation::Config(ConfigOperation::Put {
            id: resource()?,
            input: parse(body)?,
        }),
        ("POST", "/configuration-validations") => {
            HostOperation::Config(ConfigOperation::Validate(parse(body)?))
        }
        ("POST", "/configuration/applications") => {
            HostOperation::Config(ConfigOperation::Apply(parse(body)?))
        }
        ("GET", "/configuration/applications/{id}") => {
            HostOperation::Config(ConfigOperation::Application { id: resource()? })
        }
        ("GET", "/prompts/{id}") => {
            HostOperation::Config(ConfigOperation::GetPrompt { id: resource()? })
        }
        ("PUT", "/prompts/{id}") => HostOperation::Config(ConfigOperation::PutPrompt {
            id: resource()?,
            input: parse(body)?,
        }),
        ("GET", "/secrets") => HostOperation::Config(ConfigOperation::Secrets),
        ("PUT", "/secrets/{id}") => HostOperation::Config(ConfigOperation::PutSecret {
            id: resource()?,
            input: parse(body)?,
        }),
        ("DELETE", "/secrets/{id}") => {
            HostOperation::Config(ConfigOperation::DeleteSecret { id: resource()? })
        }
        ("GET", "/approvals") => HostOperation::Approval(ApprovalOperation::List),
        ("POST", "/approvals/{id}/decisions") => {
            HostOperation::Approval(ApprovalOperation::Decide {
                id: resource()?,
                input: parse(body)?,
            })
        }
        _ => return Err(ApiError::not_found()),
    };
    host_access(caller, &operation)?;
    state.host.operation(caller, operation).await
}
fn page(query: &HashMap<String, String>) -> ApiResult<PageQuery> {
    let limit = query
        .get("limit")
        .map(|value| {
            value
                .parse::<usize>()
                .map_err(|_| ApiError::invalid("Invalid page limit."))
        })
        .transpose()?
        .unwrap_or(10);
    if !(1..=100).contains(&limit) || query.get("order").is_some_and(|order| order != "desc") {
        return Err(ApiError::invalid(
            "Pages require order=desc and limit between 1 and 100.",
        ));
    }
    Ok(PageQuery {
        after: query.get("after").cloned(),
        limit: Some(limit),
        order: Some("desc".into()),
        through_id: query.get("through_id").cloned(),
    })
}

fn revision(headers: &HeaderMap) -> ApiResult<Option<String>> {
    headers
        .get(IF_MATCH)
        .map(|value| {
            let value = value
                .to_str()
                .map_err(|_| ApiError::invalid("Invalid If-Match revision."))?;
            if value.starts_with("W/") || value.contains(',') || value == "*" {
                return Err(ApiError::invalid("Use one exact revision in If-Match."));
            }
            Ok(value.trim_matches('"').to_owned())
        })
        .transpose()
}
