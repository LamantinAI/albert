use crate::db::error::{DatabaseError, DatabaseResult};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrivateRouteKey {
    pub connector_id: String,
    pub channel_id: String,
    pub user_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct PrivateRoute {
    pub session_id: String,
    pub return_session_id: Option<String>,
    pub revision: i64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum MarkerState {
    Temporary,
    Saved,
    Closed,
    Expired,
}

impl MarkerState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Temporary => "temporary",
            Self::Saved => "saved",
            Self::Closed => "closed",
            Self::Expired => "expired",
        }
    }

    pub fn parse(value: &str) -> DatabaseResult<Self> {
        match value {
            "temporary" => Ok(Self::Temporary),
            "saved" => Ok(Self::Saved),
            "closed" => Ok(Self::Closed),
            "expired" => Ok(Self::Expired),
            _ => Err(DatabaseError::MarkerState),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SessionMarker {
    pub owner_user_id: Option<String>,
    pub session_id: String,
    pub runtime_epoch: String,
    pub state: MarkerState,
    pub revision: i64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ExcursionKey {
    pub connector_id: String,
    pub bot_id: String,
    pub chat_id: String,
    pub actor_id: String,
}

/// Bounded application ordering for ordinary Telegram Message events. Dates
/// come from the outer provider message; Update IDs break ties in one second.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct NativePosition {
    pub date: i64,
    pub update_id: i64,
}

#[derive(Clone, Debug)]
pub(crate) struct Excursion {
    pub key: ExcursionKey,
    pub user_id: String,
    pub session_id: String,
    pub return_session_id: String,
    pub scope_epoch: String,
    pub start: NativePosition,
    pub start_message_id: i64,
    pub end: Option<NativePosition>,
    pub end_message_id: Option<i64>,
    pub disposition: String,
    pub revision: i64,
}
