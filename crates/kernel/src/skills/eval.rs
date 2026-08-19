use crate::prelude::*;
use anylm::api::{Schema, Tool};

pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new(
            "javascript_eval",
            "Executes JS code for exact calculations (math, date/time formatting, timezone conversions, string/array transforms) \
             instead of estimating results. Returns the evaluated value of the last expression.",
        )
        .required_property(
            "code",
            Schema::string(
                "Plain JS code. No TS types, no markdown, no console.log. \
                 The last line/expression is returned as the result.",
            ),
        )
    ]
}

#[derive(Deserialize)]
pub struct EvalAction {
    pub task_id: Option<i64>,
    pub parameter: Option<String>,
    pub code: String,
}
