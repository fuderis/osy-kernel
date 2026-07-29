use crate::prelude::*;
use anylm::api::{Schema, Tool};

/// The user context fact
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserFact {
    /// Text content of the extracted fact
    pub text: String,
    /// Unix timestamp (in seconds) when the fact was stored
    pub created_at: u64,
}

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new(
            "remember_fact",
            "Saves a new persistent fact or memory about the user into long-term memory. \
            Use this when the user explicitly asks to remember something or discloses important user-specific information \
            (e.g., preferences, personal facts, project settings, name, tech stack).",
        )
        .required_property(
            "fact",
            Schema::string("The concise fact or information to remember about the user."),
        ),

        Tool::new(
            "forget_fact",
            "Removes a specific fact from long-term memory by its ID. \
            Use this when a previously remembered fact is obsolete, incorrect, or the user explicitly asks to forget it.",
        )
        .required_property(
            "fact_id",
            Schema::integer("The unique numerical ID of the fact to forget."),
        ),
    ]
}

#[derive(Deserialize, Debug)]
pub struct RememberFactAction {
    pub fact: String,
}

#[derive(Deserialize, Debug)]
pub struct ForgetFactAction {
    pub fact_id: u64,
}
