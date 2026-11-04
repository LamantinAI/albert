use serde::Serialize;
use utoipa::ToSchema;

#[derive(Serialize, ToSchema)]
pub struct ConnectorTypeView {
    pub kind: String,
    pub label: String,
}

#[derive(Serialize, ToSchema)]
pub struct ConnectorView {
    pub id: String,
    pub kind: String,
    pub label: String,
    pub settings_mode: String,
}

#[derive(Serialize, ToSchema)]
pub struct ConnectorList {
    pub items: Vec<ConnectorView>,
    pub available_types: Vec<ConnectorTypeView>,
}
