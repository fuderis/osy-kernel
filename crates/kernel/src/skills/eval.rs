use crate::{prelude::*, runtime::Runtime};
use anylm::api::{Schema, Tool};

/// Returns tools list.
pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new(
            "js_eval",
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

/// JavaScript evaluation data.
#[derive(Deserialize)]
pub struct EvalAction {
    /// Execution code (JavaScript).
    pub code: String,
}

/// Handles JavaScript execution.
#[log()]
pub async fn handle_eval(action: EvalAction) -> Result<String> {
    info!("Executing JavaScript code: {:80}...", &action.code);

    Runtime::new()
        .eval(&action.code)
        .map_err(|e| Error::Titled("JavaScript execution error".into(), e).into())
}
