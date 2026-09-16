use crate::Skill;

use atoman::State;
use rigging::PkgMeta;
use serde::{Deserialize, Serialize};
use std::{collections::HashMap, path::PathBuf, sync::Arc};

static AGENT_METADATA: State<AgentMeta> = State::new(|| Default::default());

/// Agent metadata.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct AgentMeta {
    pub name: String,
    pub description: String,
    pub version: String,
    pub skills: HashMap<String, Skill>,
    pub sock_name: String,
    pub sock_path: PathBuf,
}

impl AgentMeta {
    pub async fn init(pkg: PkgMeta, skills: Vec<Skill>) {
        let name = pkg.name.trim_start_matches("osy-").to_string();
        let sock_name = format!("osy-{name}");

        AGENT_METADATA
            .set(Self {
                name,
                description: pkg.description.into(),
                version: pkg.version.into(),
                skills: skills.into_iter().map(|s| (s.name.clone(), s)).collect(),
                sock_path: macron::path!("$temp/{sock_name}.sock"),
                sock_name,
            })
            .await;
    }

    pub fn get() -> Arc<Self> {
        AGENT_METADATA.get()
    }

    pub fn get_cloned() -> Self {
        AGENT_METADATA.get_cloned()
    }

    pub fn skills(mut self, f: fn() -> Vec<Skill>) -> Self {
        self.skills = f()
            .into_iter()
            .map(|skill| (skill.name.clone(), skill))
            .collect();
        self
    }

    pub fn to_json_string(&self) -> String {
        serde_json::to_string(self).expect("Failed convert to JSON string.")
    }
}
