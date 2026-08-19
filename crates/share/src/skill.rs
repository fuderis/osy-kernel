use anylm::api::Tool;
use pearce::{Bytes, Sender};
use serde::{Deserialize, Serialize};
use serde_json::Value as JsonValue;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error + Sync + Send>>;

/// Global DTO structure of the skill
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Skill {
    pub name: String,
    pub description: String,
    pub prompt: String,
}

impl Skill {
    pub fn new(
        name: impl Into<String>,
        description: impl Into<String>,
        prompt: impl Into<String>,
    ) -> Self {
        Self {
            name: name.into(),
            description: description.into(),
            prompt: prompt.into(),
        }
    }
}

/// A trait for obtaining tool diagrams
pub trait SkillExt {
    fn tools_list(&self) -> Vec<Tool>;

    fn tool_call(
        &self,
        tx: Sender<Bytes>,
        tool: String,
        payload: JsonValue,
    ) -> impl Future<Output = Result<()>> + Send;
}
