use anylm::Bytes;
use serde::{Deserialize, Serialize};

/// The assistant stream event
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", content = "text", rename_all = "snake_case")]
pub enum Event {
    Thinking(String), // JSON: { "type": "thinking", "text": "..." }
    Answer(String),   // JSON: { "type": "answer", "text": "..." }
    Error(String),    // JSON: { "type": "error", "text": "..." }
    Finish,           // JSON: { "type": "thinking" }
}

impl Event {
    pub fn think(text: impl Into<String>) -> Self {
        Self::Thinking(text.into())
    }

    pub fn answer(text: impl Into<String>) -> Self {
        Self::Answer(text.into())
    }

    pub fn error(text: impl Into<String>) -> Self {
        Self::Error(text.into())
    }

    pub fn finish() -> Self {
        Self::Finish
    }
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
