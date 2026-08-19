// use crate::prelude::DynError;
use macron::{Display, Error, From};

// The error
#[derive(Debug, Display, Error, From)]
pub enum Error {
    #[display(fmt = "A non-existing tool was called: `{0}`")]
    UnknownTool(String),
}
