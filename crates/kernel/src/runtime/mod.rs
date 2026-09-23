//! JavaScript runtime module (Boa Engine).

use crate::prelude::*;

use boa_engine::{value::TryFromJs, vm::RuntimeLimits, Context, JsValue, Source};

/// JavaScript runtime executor
pub struct Runtime {
    /// Isolated runtime context
    context: Context,
}

impl Runtime {
    /// Creates new JavaScript runtime with configured limits.
    pub fn new() -> Self {
        let mut context = Context::default();
        let runtime_settings = &Config::get().runtime;

        if let Some(limit) = runtime_settings.instruction_limit {
            // creating a limit configuration
            let mut limits = RuntimeLimits::default();
            limits.set_loop_iteration_limit(limit);
            context.set_runtime_limits(limits);
        }

        Self { context }
    }

    /// Evaluates JavaScript and returns the result as a string.
    #[log()]
    pub fn eval(&mut self, code: &str) -> Result<String> {
        info!("Executing JS script: {code:80}...");

        let value = self
            .context
            .eval(Source::from_bytes(code))
            .map_err(|e| format!("JS Execution Error: {e}"))?;

        self.js_to_string(&value).map_err(|e| e.to_string().into())
    }

    /// Evaluates JavaScript and converts the result to a Rust type.
    #[log()]
    pub fn eval_json<T>(&mut self, code: &str) -> Result<T>
    where
        T: TryFromJs,
    {
        info!("Executing JS script: {code:80}...");

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

    /// Helper method to convert JS type to string
    fn js_to_string(&mut self, value: &JsValue) -> StdResult<String, boa_engine::JsError> {
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
            self.context.register_global_property(
                boa_engine::js_string!("__value__"),
                value.clone(),
                boa_engine::property::Attribute::all(),
            )?;

            let json = self
                .context
                .eval(Source::from_bytes("JSON.stringify(__value__)"))?;

            return Ok(json.to_string(&mut self.context)?.to_std_string_escaped());
        }

        Ok(value.to_string(&mut self.context)?.to_std_string_escaped())
    }
}

impl Default for Runtime {
    fn default() -> Self {
        Self::new()
    }
}
