use crate::prelude::*;

/// The server options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerOptions {
    pub max_logs: usize,
}

impl ::std::default::Default for ServerOptions {
    fn default() -> Self {
        Self { max_logs: 1000 }
    }
}

/// The agent settings
#[atoman::config]
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub server: ServerOptions,
}
