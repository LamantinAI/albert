use super::models::{McpServerWrite, McpToolQuery};
pub enum McpOperation {
    List,
    Create(McpServerWrite),
    Update {
        id: String,
        input: McpServerWrite,
    },
    Delete {
        id: String,
        revision: Option<String>,
    },
    Reconnect {
        id: String,
    },
    Refresh {
        id: String,
    },
    Tools {
        id: String,
        query: McpToolQuery,
    },
}
