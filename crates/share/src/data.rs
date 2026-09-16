use crate::AgentMeta;
use serde::{Deserialize, Serialize};

/// Server status data.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub agents_list: Vec<AgentMeta>,
}

/// Command execution results.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandResult {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i16,
    pub success: bool,
}
