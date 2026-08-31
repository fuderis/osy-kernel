use crate::Skill;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// The agent metadata
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AgentMetadata {
    pub name: String,
    pub description: String,
    pub version: String,
    pub skills: HashMap<String, Skill>,
}

impl AgentMetadata {
    pub fn skills(mut self, f: fn() -> Vec<Skill>) -> Self {
        self.skills = f()
            .into_iter()
            .map(|skill| (skill.name.clone(), skill))
            .collect();
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
            skills: map! {},
        }
        .skills(skills::skills_list)
    };
}
