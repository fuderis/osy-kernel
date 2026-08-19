pub mod info;
pub mod media;
pub mod power;
pub mod theme;

use crate::prelude::*;
use anylm::api::Tool;
use osy_share::{Skill, SkillExt};
use pearce::{Bytes, Sender};
use serde_json::from_value;

/// Returns the agent skills list
pub fn skills_list() -> Vec<Skill> {
    vec![
        Skill::new(
            str!(SkillName::Info),
            "Hardware information, live system metrics and connected devices.",
            "",
        ),
        Skill::new(
            str!(SkillName::Media),
            "Audio volume control, media playback (play/pause, stop, next/prev track) and search or play music.",
            "You can find out the user’s favorite music using the `search_fact` tool (the query format matters). \n\
            NEVER TOUCH THE VOLUME unless the user has asked you to.",
        ),
        Skill::new(
            str!(SkillName::Power),
            "Shutdown, reboot, suspend and power scheduling.",
            "",
        ),
        Skill::new(
            str!(SkillName::Theme),
            "Desktop appearance and theme management.",
            "",
        ),
    ]
}

/// The agent skill name
#[derive(Clone, Copy, Debug, Display, Serialize, Deserialize, Eq, PartialEq, Hash)]
#[serde(rename_all = "snake_case")]
#[display(rename_all = "snake_case")]
pub enum SkillName {
    Info,
    Media,
    Power,
    Theme,
}

impl SkillExt for SkillName {
    fn tools_list(&self) -> Vec<Tool> {
        match self {
            Self::Info => info::tools_list(),
            Self::Media => media::tools_list(),
            Self::Power => power::tools_list(),
            Self::Theme => theme::tools_list(),
        }
    }

    async fn tool_call(&self, tx: Sender<Bytes>, tool: String, payload: JsonValue) -> Result<()> {
        warn!("WORKING...");

        match self {
            Self::Info => match tool.as_str() {
                "get_system_info" => {
                    info::handle_system_info(tx.clone(), from_value(payload)?).await
                }
                "get_system_metrics" => {
                    info::handle_system_metrics(tx.clone(), from_value(payload)?).await
                }
                "get_devices_list" => {
                    info::handle_devices_list(tx.clone(), from_value(payload)?).await
                }
                _ => Err(Error::UnknownTool(tool).into()),
            },

            Self::Media => match tool.as_str() {
                // Audio control
                "get_volume" => media::handle_get_volume(tx.clone(), from_value(payload)?).await,
                "set_volume" => media::handle_set_volume(tx.clone(), from_value(payload)?).await,
                "increase_volume" => {
                    media::handle_increase_volume(tx.clone(), from_value(payload)?).await
                }
                "decrease_volume" => {
                    media::handle_decrease_volume(tx.clone(), from_value(payload)?).await
                }
                "is_muted" => media::handle_is_muted(tx.clone(), from_value(payload)?).await,
                "set_mute" => media::handle_set_mute(tx.clone(), from_value(payload)?).await,

                // Media playback
                "media_play" => media::handle_media_play(tx.clone(), from_value(payload)?).await,
                "media_pause" => media::handle_media_pause(tx.clone(), from_value(payload)?).await,
                "media_play_pause" => {
                    media::handle_media_play_pause(tx.clone(), from_value(payload)?).await
                }
                "media_stop" => media::handle_media_stop(tx.clone(), from_value(payload)?).await,
                "media_next_track" => {
                    media::handle_media_next_track(tx.clone(), from_value(payload)?).await
                }
                "media_previous_track" => {
                    media::handle_media_previous_track(tx.clone(), from_value(payload)?).await
                }
                "media_seek_forward" => {
                    media::handle_media_seek_forward(tx.clone(), from_value(payload)?).await
                }
                "media_seek_backward" => {
                    media::handle_media_seek_backward(tx.clone(), from_value(payload)?).await
                }
                "media_metadata" => {
                    media::handle_media_metadata(tx.clone(), from_value(payload)?).await
                }
                "media_position" => {
                    media::handle_media_position(tx.clone(), from_value(payload)?).await
                }
                "media_duration" => {
                    media::handle_media_duration(tx.clone(), from_value(payload)?).await
                }

                // Music indexer
                "search_music" => {
                    media::handle_search_music(tx.clone(), from_value(payload)?).await
                }
                "play_music" => media::handle_play_music(tx.clone(), from_value(payload)?).await,

                _ => Err(Error::UnknownTool(tool).into()),
            },

            Self::Power => match tool.as_str() {
                "schedule_power" => {
                    power::handle_schedule_power(tx.clone(), from_value(payload)?).await
                }
                "cancel_power" => {
                    power::handle_cancel_power(tx.clone(), from_value(payload)?).await
                }
                "get_power_status" => {
                    power::handle_power_status(tx.clone(), from_value(payload)?).await
                }
                _ => Err(Error::UnknownTool(tool).into()),
            },

            Self::Theme => match tool.as_str() {
                "set_theme" => theme::handle_set_theme(tx.clone(), from_value(payload)?).await,
                // TODO: "get_theme" => theme::handle_get_theme(tx.clone(), from_value(payload)?).await,
                _ => Err(Error::UnknownTool(tool).into()),
            },
        }
    }
}
