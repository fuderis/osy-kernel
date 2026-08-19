use crate::prelude::DynError;
use macron::{Display, Error, From};
use osy_share::SessionId;

// The error
#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display(fmt = "Failed to get agent name")]
    FailedGetAgentName,

    #[display(fmt = "Failed to fetch agent metadata for {name}: {source}")]
    FailedFetchMetadata { name: String, source: DynError },

    #[display(fmt = "Agent `{name}` failed to start on sock {sock_path} after 10 attempts.")]
    AgentStartFailed { name: String, sock_path: String },

    #[display(fmt = "Failed to parse AgentInfo response payload: {0}")]
    AgentInfoParsingFailed(#[source] DynError),

    #[display(fmt = "Unknown session id {0} has been received")]
    UnknownSessionId(SessionId),

    #[display(fmt = "The TypeScript runtime is not initialized, check logs")]
    RuntimeNotInitialized,

    #[display(fmt = "No embedding received from provider")]
    NoEmbeddingReceived,

    #[display(fmt = "Invalid skill format in handle_task: {0}")]
    InvalidSkillNameFormat(String),
}
