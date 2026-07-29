use crate::prelude::*;

use anylm::api::{Schema, Tool};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new(
                "handle_agent",
                "Delegates a task to a specific AI agent for execution (do not invent non-existent agents).",
            )
            .required_property(
                "agent_name",
                Schema::string("The name of the agent to handle this task."),
            )
            .required_property(
                "agent_skills",
                Schema::array("The agent skills required to complete the task.")
                    .items(Schema::string("The skill identifier."))
            )
            .required_property(
                "task_id",
                Schema::integer("An unique identifier for the task (starting from 1, and should not be repeated)."),
            )
            .required_property(
                "task_query",
                Schema::string("The task query and data for the agent handling (describe the task in details)."),
            )
            .optional_property(
                "depend_tasks",
                Schema::array("Identifiers of tasks that must be completed before this one (when need the results of another tasks).")
                    .items(Schema::integer("Identifier of task that must be completed before."))
            )
    ]
}

/// The agent task info
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TaskAction {
    #[serde(default = "TaskAction::random_id")]
    pub task_id: i64,
    #[serde(default)]
    pub tool_call_id: String,
    pub agent_name: String,
    pub agent_skills: Vec<String>,
    pub task_query: String,
    #[serde(default)]
    pub depend_tasks: HashSet<i64>,
}

impl TaskAction {
    /// Generates the random task ID
    fn random_id() -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos() as i64
    }
}
