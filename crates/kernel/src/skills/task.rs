use crate::{handlers, prelude::*};

use anylm::api::{Content, Message, Messages, Schema, Tool};
use osy_share::SessionInfo;

/// Returns tools list.
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

/// Agent task data.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TaskAction {
    #[serde(default)]
    pub tool_call_id: String,

    /// Target skill name.
    pub skill: String,
    /// Agent task query.
    pub query: String,
}

/// Handles agent task (isolated).
#[log(skill = %task.skill)]
pub async fn handle_task(
    tx: Sender<Bytes>,
    session_info: SessionInfo,
    messages: Arc<Mutex<Messages>>,
    task: TaskAction,
) -> Result<()> {
    let skill_response = handlers::handle_skill(
        tx,
        session_info,
        &task.skill,
        Message::user(vec![task.query.into()]),
    )
    .await?;

    // recording result of skill execution in the parent message context
    messages
        .lock()
        .await
        .push_content(Some(&task.tool_call_id), Content::text(skill_response));

    Ok(())
}
