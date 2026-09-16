//! Helper methods module.

pub mod info;
pub use info::*;

pub mod context;
pub use context::*;

pub mod prompt;
pub use prompt::*;

pub mod sudo;
pub use sudo::*;

use crate::prelude::*;
use chrono::FixedOffset;

/// Returns session local date time
pub fn now_local(timezone_m: i16) -> DateTime<FixedOffset> {
    let offset_seconds = (timezone_m as i32) * 60;
    let tz =
        FixedOffset::east_opt(offset_seconds).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

    let utc_now = Utc::now();
    utc_now.with_timezone(&tz)
}

/// Returns true if english text
pub fn is_english(text: &str) -> bool {
    whatlang::detect(text)
        .map(|info| info.lang() == whatlang::Lang::Eng)
        .unwrap_or(false)
}
