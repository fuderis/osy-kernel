use crate::prelude::*;
use boa_engine::{Context, JsValue, Source, value::TryFromJs, vm::RuntimeLimits};

pub struct Runtime {
    context: Context,
}

impl Runtime {
    /// Creates a new JavaScript runtime with configured limits.
    pub fn new() -> Self {
        let mut context = Context::default();
        let runtime_settings = &Settings::get().runtime;

        if let Some(limit) = runtime_settings.instruction_limit {
            // creating a limit configuration
            let mut limits = RuntimeLimits::default();
            limits.set_loop_iteration_limit(limit);
            context.set_runtime_limits(limits);
        }

        Self { context }
    }

    /// Evaluates JavaScript and returns the result as a string.
    pub fn eval(&mut self, code: &str) -> Result<String> {
        let value = self
            .context
            .eval(Source::from_bytes(code))
            .map_err(|e| format!("JS Execution Error: {e}"))?;

        js_to_string(&value, &mut self.context).map_err(|e| e.to_string().into())
    }

    /// Evaluates JavaScript and converts the result to a Rust type.
    pub fn eval_json<T>(&mut self, code: &str) -> Result<T>
    where
        T: TryFromJs,
    {
        let value = self
            .context
            .eval(Source::from_bytes(code))
            .map_err(|e| format!("JS Execution Error: {e}"))?;

        T::try_from_js(&value, &mut self.context).map_err(|e| e.to_string().into())
    }

    /// Clears the runtime by creating a fresh Context with default settings.
    pub fn reset(&mut self) {
        *self = Self::new();
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}

fn js_to_string(value: &JsValue, ctx: &mut Context) -> StdResult<String, boa_engine::JsError> {
    if value.is_null() {
        return Ok("null".into());
    }

    if value.is_undefined() {
        return Ok("undefined".into());
    }

    if let Some(s) = value.as_string() {
        return Ok(s.to_std_string_escaped());
    }

    if value.is_object() {
        ctx.register_global_property(
            boa_engine::js_string!("__osy_value__"),
            value.clone(),
            boa_engine::property::Attribute::all(),
        )?;

        let json = ctx.eval(Source::from_bytes("JSON.stringify(__osy_value__)"))?;

        return Ok(json.to_string(ctx)?.to_std_string_escaped());
    }

    Ok(value.to_string(ctx)?.to_std_string_escaped())
}
