#![allow(unused_imports)]

// Domain crates
pub use crate::{error::Error, settings::Settings};
pub use osy_share::{Id, SessionId};

// Standard library primitives
pub use std::{
    collections::{HashMap, HashSet},
    path::{Path, PathBuf},
    pin::Pin,
    result::Result as StdResult,
    sync::Arc,
    time::Duration,
};
pub use tokio::{
    sync::{Mutex, RwLock},
    time::Instant,
};

// Ecosystem crates
pub use atoman::{
    Config, Dir, File, Instrument, Logger, SharedGuard, SharedGuardMut, SharedItem, SharedMap,
    Span, State, StateGuard, error, info, log, warn,
};
pub use chrono::{DateTime, Local, Utc};
pub use macron::{Display, From, arc, arc_mutex, async_recursion, path, str};
pub use pearce::{Bytes, Client, Json, Paths, Query, Receiver, Response, Sender, StreamExt};

// Serialization
pub use serde::{Deserialize, Serialize};
pub use serde_json::{self as json, Value as JsonValue, json};

// Error handling
pub type DynError = Box<dyn std::error::Error + Send + Sync>;
pub type Result<T> = StdResult<T, DynError>;
