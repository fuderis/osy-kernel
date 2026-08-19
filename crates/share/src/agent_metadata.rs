use crate::Skill;
use serde::{Deserialize, Serialize};

/// The agent metadata
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    pub skills: Vec<Skill>,
}

impl AgentMetadata {
    pub fn skills(mut self, f: fn() -> Vec<Skill>) -> Self {
        self.skills = f();
        self
    }
}

#[macro_export]
macro_rules! agent_metadata {
    () => {
        $crate::AgentMetadata {
            name: env!("CARGO_PKG_NAME").trim_start_matches("osy-").into(),
            description: env!("CARGO_PKG_DESCRIPTION").into(),
            version: env!("CARGO_PKG_VERSION").into(),
            skills: vec![],
        }
        .skills(skills::skills_list)
    };
}
