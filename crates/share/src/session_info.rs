use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User session info.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// System info (OS type & version).
    pub system_info: Option<String>,
    /// Working directory (where CLI started).
    pub current_path: Option<PathBuf>,
    /// Client timezone (offset in minutes).
    pub timezone: i16,
}

impl Default for SessionInfo {
    fn default() -> Self {
        Self {
            system_info: None,
            current_path: None,
            timezone: 180,
        }
    }
}
