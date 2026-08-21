use crate::prelude::*;

/// The session table key
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Key {
    Metadata,
    Message(usize),
}
