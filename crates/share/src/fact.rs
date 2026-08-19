use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// The user context fact stored in Sled database
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserFact {
    /// Internal unique identifier matching LanceDB entry
    pub id: u64,
    /// Raw text content of the fact
    pub text: String,
    /// Text optimized for vector search/embeddings
    pub search_text: String,
    /// Unix timestamp (in seconds, UTC) when the fact was stored
    pub created_at: DateTime<Utc>,
}
