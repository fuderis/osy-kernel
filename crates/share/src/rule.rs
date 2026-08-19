use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Rule model representing dynamic system instructions/directives
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserRule {
    /// Unique ID
    pub id: u64,
    /// Rule prompt payload
    pub text: String,
    /// Flag indicating whether the rule applies globally across all user sessions
    pub is_global: bool,
    /// Timestamp when the rule was created
    pub created_at: DateTime<Utc>,
}
