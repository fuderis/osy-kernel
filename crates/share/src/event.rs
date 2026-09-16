use anylm::Bytes;
use rigging::widgets::Confirmation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum DialogEvent {
    Script {
        id: String,
        code: String,
    },
    Confirm {
        id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<Confirmation>,
    },
    Prompt {
        id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        multiline: Option<bool>,
    },
    Secret {
        id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Select {
        id: String,
        prompt: String,
        items: Vec<String>,
    },
}

impl DialogEvent {
    /// Returns dialog event ID.
    pub fn get_id(&self) -> &str {
        match self {
            Self::Confirm { id, .. } => id,
            Self::Prompt { id, .. } => id,
            Self::Secret { id, .. } => id,
            Self::Select { id, .. } => id,
            Self::Script { id, .. } => id,
        }
    }
}

/// Assistant stream event
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum Event {
    Thinking(String),
    Answer(String),
    Error(String),
    Dialog(DialogEvent),
    Finish,
}

impl Into<String> for Event {
    fn into(self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

impl Into<Bytes> for Event {
    fn into(self) -> Bytes {
        Into::<String>::into(self).into()
    }
}
