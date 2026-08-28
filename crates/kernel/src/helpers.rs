use crate::prelude::*;

use anylm::api::{Content, Message};
use chrono::FixedOffset;

/// Returns the session local date time
pub fn now_local(timezone_m: i16) -> DateTime<FixedOffset> {
    let offset_seconds = (timezone_m as i32) * 60;
    let tz =
        FixedOffset::east_opt(offset_seconds).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

    let utc_now = Utc::now();
    utc_now.with_timezone(&tz)
}

/// Extracts a text from the message
pub fn extract_text_from_msg(msg: &Message) -> Option<String> {
    let text: String = msg
        .content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}

/// Returns true if english text
pub fn is_english(text: &str) -> bool {
    whatlang::detect(text)
        .map(|info| info.lang() == whatlang::Lang::Eng)
        .unwrap_or(false)
}
