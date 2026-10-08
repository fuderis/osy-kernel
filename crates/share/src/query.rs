use anylm::api::Message;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListQuery {
    #[serde(default)]
    pub count: Option<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleQuery {
    pub current_path: Option<PathBuf>,
    pub message: Message,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolQuery<T> {
    pub current_path: Option<PathBuf>,
    #[serde(flatten)]
    pub payload: T,
}

impl ToolQuery<serde_json::Value> {
    pub fn parse_payload<T: serde::de::DeserializeOwned>(
        self,
    ) -> Result<ToolQuery<T>, serde_json::Error> {
        let typed_payload = serde_json::from_value(self.payload)?;
        Ok(ToolQuery {
            current_path: self.current_path,
            payload: typed_payload,
        })
    }
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
pub struct RenameQuery {
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SearchQuery {
    pub query: String,
    #[serde(default)]
    pub limit: Option<usize>,
}
