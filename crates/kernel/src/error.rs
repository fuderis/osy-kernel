use crate::prelude::DynError;
use macron::{Display, Error, From};
use osy_share::SessionId;

/// Kernel error during execution.
#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display(fmt = "{0}")]
    Custom(String),

    #[display(fmt = "{0}: {1}")]
    Titled(String, #[source] DynError),

    #[display(fmt = "Connection to the client is closed.")]
    ConnectionClosed,

    #[display(fmt = "Failed to fetch agent file name.")]
    FetchAgentFileName,

    #[display(fmt = "Failed to fetch agent metadata for {name}: {source}.")]
    FetchAgentMetadata { name: String, source: DynError },

    #[display(fmt = "Agent `{name}` failed to launch on socket `{sock_path}`.")]
    AgentNotLaunched { name: String, sock_path: String },

    #[display(fmt = "Unknown session id `{0}` has been received.")]
    UnknownSessionId(SessionId),

    #[display(fmt = "No embedding received from provider.")]
    NoEmbeddingReceived,
}
