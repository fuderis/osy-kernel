use crate::{prelude::*, runtime::Runtime};

use anylm::{Schema, Tool};

/// Returns tools list.
pub fn tools_list() -> Vec<Tool> {
    vec![Tool::typed::<EvalAction>(
        "js_eval",
        "Executes JS code for exact calculations (math, date/time formatting, timezone conversions, string/array transforms) \
             instead of estimating results. Returns the evaluated value of the last expression.",
    )]
}

/// JavaScript evaluation data.
#[derive(Deserialize, Schema)]
pub struct EvalAction {
    /// Plain JS code. No TS types, no markdown, no console.log. (The last line/expression is returned as the result).
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
