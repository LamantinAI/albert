use octo_history::{JournalKeyInventory, PageRequest, Role, SessionOrigin, SqliteHistory};
use serde::Deserialize;
use serde_json::from_str;
use sqlx::{Sqlite, Transaction};

use crate::sessions::error::SessionResult;

#[derive(Deserialize)]
struct Header {
    run_id: String,
    parent_run_id: String,
    origin_connector: String,
    origin_channel: String,
    assignment: Assignment,
}

/// The host serializes its Task object here. Decode only the historical task
/// text; model/tool options from an old release are not current input policy.
#[derive(Deserialize)]
#[serde(untagged)]
enum Assignment {
    Text(String),
    Task { task: String },
}

impl Assignment {
    fn is_empty(&self) -> bool {
        match self {
            Self::Text(text) | Self::Task { task: text } => text.trim().is_empty(),
        }
    }
}

/// Read only the first host header of the reserved old child namespace. User
/// text, later JSON records and the root transcript are not ownership evidence.
pub(super) struct Child {
    pub origin: SessionOrigin,
    pub run_id: String,
    pub source: JournalKeyInventory,
}

pub(super) async fn children_in(
    history: &SqliteHistory,
    tx: &mut Transaction<'_, Sqlite>,
    inventory: &[JournalKeyInventory],
) -> SessionResult<Vec<Child>> {
    let mut children = Vec::new();
    for source in inventory
        .iter()
        .filter(|source| source.key.starts_with("subagent/"))
    {
        let Some(first_id) = source.first_id else {
            continue;
        };
        let page = history
            .journal_page_in(
                tx,
                &source.key,
                PageRequest {
                    through_id: Some(first_id),
                    limit: 1,
                    ..PageRequest::default()
                },
            )
            .await?;
        let Some(first) = page
            .records
            .first()
            .filter(|record| record.id == first_id && record.turn.role == Role::User)
        else {
            continue;
        };
        let Ok(header) = from_str::<Header>(&first.turn.content) else {
            continue;
        };
        if header.run_id.len() > 2048
            || source.key != format!("subagent/{}", header.run_id)
            || !header.run_id.starts_with("albert/child-")
            || !header.parent_run_id.starts_with("albert/")
            || header.parent_run_id == header.run_id
            || header.assignment.is_empty()
        {
            continue;
        }
        children.push(Child {
            origin: SessionOrigin {
                connector: header.origin_connector,
                channel: header.origin_channel,
            },
            run_id: header.run_id,
            source: source.clone(),
        });
    }
    Ok(children)
}
