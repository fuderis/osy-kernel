use anylm::api::Message;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleQuery {
    pub message: Message,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactQuery {
    #[serde(default)]
    pub preserve: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SetQuery {
    #[serde(default)]
    pub id: Option<u64>,
    pub text: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoveQuery {
    pub id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
}
