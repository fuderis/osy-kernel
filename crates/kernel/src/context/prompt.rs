use crate::{helpers, prelude::*};
use osy_share::SessionInfo;

/// Generates the system prompt
pub fn system_prompt(info: &SessionInfo, settings: &Settings) -> String {
    let now_utc = Utc::now();
    let now_local = helpers::now_local(info.timezone);

    settings
        .completions
        .system_prompt
        .trim()
        .replace(
            "{DATETIME_LOCAL}",
            &now_local
                .format("%A, %B %d, %Y, %I:%M:%S %p %Z")
                .to_string(),
        )
        .replace(
            "{DATETIME_GLOBAL}",
            &now_utc.format("%A, %B %d, %Y, %I:%M:%S %p UTC").to_string(),
        )
        .replace(
            "{CURRENT_PATH}",
            &info
                .current_path
                .clone()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
        )
}
