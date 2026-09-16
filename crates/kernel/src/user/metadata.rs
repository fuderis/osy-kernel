use crate::prelude::*;

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
    /// Total session messages count.
    pub message_count: usize,
    /// Point of the last compressed segment.
    pub compressed_until: usize,
}
