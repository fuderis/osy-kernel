use crate::{prelude::*, utils};

use osy_share::SessionInfo;

// Returns local session info (for CLI chat).
pub fn session_info() -> SessionInfo {
    let tz_minutes = (chrono::Local::now().offset().local_minus_utc() / 60) as i16;
    SessionInfo {
        system_info: Some(utils::system_info_cloned()),
        current_path: std::env::current_dir().ok(),
        timezone: tz_minutes,
    }
}

/// Generates the base system prompt
pub fn system_prompt(info: &SessionInfo, cfg: &Config) -> String {
    let now_utc = Utc::now();
    let now_local = utils::now_local(info.timezone);

    cfg.prompts
        .system_prompt
        .trim()
        .replace(
            "{SYSTEM_INFO}",
            &info
                .system_info
                .as_ref()
                .map(|s| s.as_str())
                .unwrap_or("Unknown OS"),
        )
        .replace(
            "{CURRENT_PATH}",
            &info
                .current_path
                .as_ref()
                .map(|path| path.to_string_lossy())
                .unwrap_or("Unknown".into()),
        )
        .replace(
            "{DATETIME_GLOBAL}",
            &now_utc.format("%A, %B %d, %Y, %I:%M:%S %p UTC").to_string(),
        )
        .replace(
            "{DATETIME_LOCAL}",
            &now_local
                .format("%A, %B %d, %Y, %I:%M:%S %p %Z")
                .to_string(),
        )
}
