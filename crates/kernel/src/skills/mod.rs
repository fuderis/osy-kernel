//! Embedded skills module.

pub mod eval;
pub use eval::{EvalAction, handle_eval};

pub mod fact;
pub use fact::{handle_remember_fact, handle_search_fact};

pub mod task;
pub use task::{TaskAction, handle_task};
