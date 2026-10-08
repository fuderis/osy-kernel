use crate::{handlers, prelude::*};

use anylm::{Content, Message, Messages, Schema, Tool};
use osy_share::SessionInfo;

/// Returns tools list.
pub fn tools_list() -> Vec<Tool> {
    vec![Tool::typed::<SkillAction>(
        "use_skill",
        "Delegates a task using a specific skill.",
    )]
}

/// Agent skill task data.
#[derive(Deserialize, Schema)]
pub struct SkillAction {
    #[schema(skip)]
    #[serde(default)]
    pub tool_call_id: String,

    /// The existing skill required for this task.
    pub skill: String,
    /// The detailed task prompt, parameters, and input context.
    pub query: String,
}

/// Handles agent skill (isolated).
#[log(skill = %task.skill)]
pub async fn handle_skill(
    tx: Sender<Bytes>,
    session_info: SessionInfo,
    messages: Arc<Mutex<Messages>>,
    current_path: Option<PathBuf>,
    task: SkillAction,
) -> Result<()> {
    let skill_response = handlers::handle_skill(
        tx,
        session_info,
        &task.skill,
        Message::user(vec![task.query.into()]),
        current_path,
        false,
    )
    .await?;

    // recording result of skill execution in the parent message context
    messages
        .lock()
        .await
        .push_content(Some(&task.tool_call_id), Content::text(skill_response));

    Ok(())
}
