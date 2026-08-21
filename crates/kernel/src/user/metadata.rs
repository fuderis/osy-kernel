use crate::prelude::*;

/// Global metadata persisted per user in `$share$/users/{uid}/userdata`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserMetadata {
    /// List of all created session IDs for this user
    pub sessions: Vec<SessionId>,
    /// The ID of the most recently active or created session
    pub last_session: Option<SessionId>,
}

/// The session metadata
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct SessionMetadata {
    pub session_id: SessionId,
    pub message_count: u64,
    pub compressed_until: usize,
}

impl SessionMetadata {
    /// Creates a new session metadata by session id
    pub fn new(session_id: SessionId) -> Self {
        Self {
            session_id,
            message_count: 0,
            compressed_until: 0,
        }
    }
}
