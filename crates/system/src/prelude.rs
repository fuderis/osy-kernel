#![allow(unused_imports)]

// Domain crates
pub use crate::{error::Error, settings::Settings};
pub use osy_share::{DialogEvent, Event, Id};

// Ecosystem crates
pub use atoman::{Config, Logger, Span, State, StateGuard, error, info, log, warn};
pub use chrono::{DateTime, Local, Utc};
pub use macron::*;
pub use pearce::{Bytes, Callback, Json, Paths, Response, Sender};
pub use rigging::widgets::Confirmation;

// Standard library primitives
pub use std::{
    path::{Path, PathBuf},
    result::Result as StdResult,
    sync::Arc,
    time::Duration,
};
pub use tokio::time::Instant;

// Serialization
pub use serde::{Deserialize, Serialize};
pub use serde_json::{self as json, Value as JsonValue, json};

// Error handling
pub type DynError = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T> = StdResult<T, DynError>;
