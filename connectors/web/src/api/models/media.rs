use serde::Serialize;
use utoipa::{
    openapi::schema::KnownFormat, openapi::schema::ObjectBuilder, openapi::schema::SchemaFormat,
    openapi::schema::Type, openapi::RefOr, openapi::Schema, PartialSchema, ToSchema,
};

#[derive(Clone, Serialize, ToSchema)]
pub struct AttachmentView {
    pub id: String,
    pub filename: String,
    pub content_type: String,
    pub size: usize,
}

/// Binary HTTP content, independent of any JSON attachment metadata.
pub struct BinaryContent;
impl PartialSchema for BinaryContent {
    fn schema() -> RefOr<Schema> {
        ObjectBuilder::new()
            .schema_type(Type::String)
            .format(Some(SchemaFormat::KnownFormat(KnownFormat::Binary)))
            .into()
    }
}
impl ToSchema for BinaryContent {}
