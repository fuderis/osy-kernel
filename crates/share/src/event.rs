use anylm::{Bytes, api::ToolCall};
use serde::{Deserialize, Serialize};

/// The event kind
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum EventKind {
    Start,
    Thinking,
    Answer,
    Error,
    Finish,
}

/// The event task info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EventTaskInfo {
    pub task_id: i64,
    pub tool_call_id: String,
}

/// The assistant event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    pub kind: EventKind,
    pub task_info: Option<EventTaskInfo>,
    pub text: String,
}

impl Event {
    /// Creates a new chunk
    pub fn new(kind: EventKind, text: impl Into<String>) -> Self {
        Self {
            kind,
            task_info: None,
            text: text.into(),
        }
    }

    /// Sets the event task info
    pub fn task_info(mut self, task: EventTaskInfo) -> Self {
        self.task_info.replace(EventTaskInfo {
            task_id: task.task_id,
            tool_call_id: task.tool_call_id.clone(),
        });
        self
    }

    pub fn raw_task_info(mut self, task_id: i64, tool_call_id: impl Into<String>) -> Self {
        self.task_info.replace(EventTaskInfo {
            task_id,
            tool_call_id: tool_call_id.into(),
        });
        self
    }

    /// Creates a start event
    pub fn start(tool_calls: &Vec<ToolCall>) -> Self {
        Self::new(
            EventKind::Start,
            serde_json::to_string(tool_calls).unwrap(), // SAFETY
        )
    }

    /// Creates a thinking event
    pub fn think(text: impl Into<String>) -> Self {
        Self::new(EventKind::Thinking, text)
    }

    /// Creates an answer chunk
    pub fn answer(text: impl Into<String>) -> Self {
        Self::new(EventKind::Answer, text)
    }

    /// Creates an error chunk
    pub fn error(text: impl Into<String>) -> Self {
        Self::new(EventKind::Error, text)
    }

    /// Creates a final agent chunk
    pub fn finish() -> Self {
        Self::new(EventKind::Finish, "")
    }

    /// Converts the chunk to string
    pub fn to_string(&self) -> String {
        // SAFETY: will be never panic
        serde_json::to_string(&self).unwrap()
    }

    /// Converts the chunk to bytes
    pub fn to_bytes(&self) -> Bytes {
        self.to_string().into()
    }
}

impl Into<String> for Event {
    fn into(self) -> String {
        self.to_string()
    }
}

impl Into<Bytes> for Event {
    fn into(self) -> Bytes {
        self.to_bytes()
    }
}
