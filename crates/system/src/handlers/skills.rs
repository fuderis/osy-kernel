use crate::{
    prelude::*,
    skills::{self, SkillName},
};
use osy_share::SkillExt;

/// API: Handles the skills list receiving
#[log(skip_all)]
pub async fn handle_skills_list() -> Response {
    let skills = skills::skills_list();
    Response::ok().json(&skills)
}

/// API: Handles the tools list receiving
#[log(skip_all)]
pub async fn handle_tools_list(skill: Paths<SkillName>) -> Response {
    let tools = skill.tools_list();
    Response::ok().json(&tools)
}

/// API: Handles the agent tool call
#[log(skip_all, fields(skill = %paths.0, tool = %paths.1))]
pub async fn handle_tool_call(
    Paths(paths): Paths<(SkillName, String)>,
    payload: Json<JsonValue>,
) -> Response {
    let (skill, tool) = paths;
    info!("Initialized the `{skill}.{tool}` tool handling");

    Response::ok().stream(async move |tx| {
        if let Err(e) = skill.tool_call(tx.clone(), tool, payload.0).await {
            error!("{e}");
            tx.send(Event::error(e.to_string())).ok();
        }
    })
}
