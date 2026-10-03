use crate::SessionId;

use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// User metadata.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct UserMetadata {
    /// All session IDs list.
    pub sessions: Vec<SessionId>,
    /// Last recently active session ID.
    pub last_session: Option<SessionId>,
}

/// Session metadata.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SessionMetadata {
    /// Session identifier.
    pub session_id: SessionId,
    /// Session title.
    pub title: Option<String>,
    /// Total session messages count.
    pub message_count: usize,
    /// Point of the last compressed segment.
    pub compressed_until: usize,
}

/// User session info.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct SessionInfo {
    /// Session identifier.
    pub id: SessionId,
    /// System info (OS type & version).
    pub system_info: Option<String>,
    /// Working directory (where CLI started).
    pub current_path: Option<PathBuf>,
    /// Client timezone (offset in minutes).
    pub timezone: i16,
}
