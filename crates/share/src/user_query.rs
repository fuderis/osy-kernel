use anylm::api::Message;
use serde::{Deserialize, Serialize};

/// The user sessions list query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserSessionsQuery {
    #[serde(default)]
    pub limit: usize,
}

impl UserSessionsQuery {
    pub fn new(limit: usize) -> Self {
        Self { limit }
    }
}

/// The session query data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandleQuery {
    pub message: Message,
}

impl HandleQuery {
    pub fn new(message: Message) -> Self {
        Self { message }
    }
}

/// The session compact data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CompactQuery {
    #[serde(default)]
    pub preserve: Option<usize>,
}

impl CompactQuery {
    pub fn new(preserve: usize) -> Self {
        Self {
            preserve: Some(preserve),
        }
    }
}
