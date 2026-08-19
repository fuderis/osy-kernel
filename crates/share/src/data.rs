use crate::AgentMetadata;
use serde::{Deserialize, Serialize};

/// The server status data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusData {
    pub agents_list: Vec<AgentMetadata>,
}
