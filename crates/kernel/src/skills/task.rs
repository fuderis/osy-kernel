// use crate::prelude::*;

use anylm::api::{Schema, Tool};
use serde::{Deserialize, Serialize};

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new("handle_task", "Delegates a task using a specific skill.")
            .required_property(
                "skill",
                Schema::string("The existing skill required for this task."),
            )
            .required_property(
                "query",
                Schema::string("The detailed task prompt, parameters, and input context."),
            ),
    ]
}

/// The task action info
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TaskAction {
    #[serde(default)]
    pub tool_call_id: String,
    pub skill: String,
    pub query: String,
}
