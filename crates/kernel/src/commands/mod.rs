//! CLI commands module.

pub mod chat;
pub use chat::*;

pub mod health;
pub use health::*;

pub mod server;
pub use server::*;

pub mod agent;
pub use agent::*;

pub mod trace;
pub use trace::*;

pub mod exec;
pub use exec::*;
