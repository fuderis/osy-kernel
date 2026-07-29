use crate::prelude::*;
use anylm::api::{Schema, Tool};
use system_utils::AudioControl;

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //              SET VOLUME
        Tool::new(
            "set_volume",
            "Sets the system audio volume to the specified percentage.",
        )
        .required_property(
            "volume",
            Schema::integer("Target audio volume percentage (0-100)."),
        ),
        Tool::new(
            "increase_volume",
            "Increases the system audio volume by the specified percentage.",
        )
        .required_property(
            "amount",
            Schema::integer("Amount to increase the audio volume by."),
        ),
        Tool::new(
            "decrease_volume",
            "Decreases the system audio volume by the specified percentage.",
        )
        .required_property(
            "amount",
            Schema::integer("Amount to decrease the audio volume by."),
        ),
        // ________________________________________
        //              GET VOLUME
        Tool::new(
            "get_volume",
            "Returns the current system audio volume percentage (0-100).",
        ),
        // ________________________________________
        //              MUTE/UNMUTE
        Tool::new(
            "is_muted",
            "Checks if the system audio is currently muted. Returns a boolean representation.",
        ),
        Tool::new(
            "set_mute",
            "Mutes or unmutes the system audio based on the provided boolean flag.",
        )
        .required_property(
            "mute",
            Schema::boolean("True to mute the audio, false to unmute it."),
        ),
    ]
}

#[derive(Deserialize)]
pub struct SetVolumeAction {
    volume: u32,
}

#[derive(Deserialize)]
pub struct DeltaVolumeAction {
    amount: u32,
}

#[log(skip_all, fields(action))]
pub async fn handle_set_volume(tx: Sender<Bytes>, action: SetVolumeAction) -> Result<()> {
    match AudioControl::set_volume(action.volume as u32).await {
        Ok(_) => {
            let msg = str!(
                "Audio volume updated successfully. Current volume: {}%.",
                action.volume
            );
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to update audio volume: {e:?}").into()),
    }
}

#[log(skip_all, fields(action))]
pub async fn handle_increase_volume(tx: Sender<Bytes>, action: DeltaVolumeAction) -> Result<()> {
    match AudioControl::increase_volume(action.amount).await {
        Ok(volume) => {
            let msg = str!("The audio volume increased successfully. Current volume: {volume}%.",);
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log(skip_all, fields(action))]
pub async fn handle_decrease_volume(tx: Sender<Bytes>, action: DeltaVolumeAction) -> Result<()> {
    match AudioControl::decrease_volume(action.amount).await {
        Ok(volume) => {
            let msg = str!("The audio volume decreased successfully. Current volume: {volume}%.",);
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to update the audio volume: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_get_volume(tx: Sender<Bytes>) -> Result<()> {
    match AudioControl::get_volume().await {
        Ok(volume) => {
            let msg = str!("The current audio volume level is {volume}%.");
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to get the audio volume: {e:?}").into()),
    }
}

#[derive(Deserialize)]
pub struct MuteAction {
    mute: bool,
}

#[log(skip_all, fields(action))]
pub async fn handle_set_mute(tx: Sender<Bytes>, action: MuteAction) -> Result<()> {
    match AudioControl::set_mute(action.mute).await {
        Ok(_) => {
            let msg = if action.mute {
                "The audio muted successfully."
            } else {
                "The audio unmuted successfully."
            };
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to update audio mute state: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_is_muted(tx: Sender<Bytes>) -> Result<()> {
    match AudioControl::is_muted().await {
        Ok(is_muted) => {
            let msg = if is_muted {
                "The audio is currently muted."
            } else {
                "The audio is currently unmuted."
            };
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to get audio mute state: {e:?}").into()),
    }
}
