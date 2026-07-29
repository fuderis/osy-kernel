use crate::prelude::*;
use anylm::api::{Schema, Tool};
use system_utils::MediaControl;

pub fn tools_list() -> Vec<Tool> {
    vec![
        #[cfg(target_os = "linux")]
        Tool::new("media_play", "Starts media playback."),
        #[cfg(target_os = "linux")]
        Tool::new("media_pause", "Pauses media playback."),
        Tool::new("media_play_pause", "Toggles between play and pause."),
        Tool::new("media_stop", "Stops media playback."),
        Tool::new("media_next_track", "Skips to the next track."),
        Tool::new("media_previous_track", "Returns to the previous track."),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_seek_forward",
            "Seeks forward by the specified number of seconds.",
        )
        .required_property(
            "seconds",
            Schema::integer("Number of seconds to seek forward."),
        ),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_seek_backward",
            "Seeks backward by the specified number of seconds.",
        )
        .required_property(
            "seconds",
            Schema::integer("Number of seconds to seek backward."),
        ),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_metadata",
            "Returns metadata for the currently playing media.",
        ),
        #[cfg(target_os = "linux")]
        Tool::new("media_position", "Returns the current playback position."),
        #[cfg(target_os = "linux")]
        Tool::new(
            "media_duration",
            "Returns the duration of the current media.",
        ),
    ]
}

#[derive(Deserialize)]
pub struct SeekAction {
    seconds: u32,
}

#[cfg(target_os = "linux")]
#[log(skip_all)]
pub async fn handle_media_play(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::play().await {
        Ok(_) => {
            let msg = "Media playback started successfully.";
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to start media playback: {e:?}").into()),
    }
}

#[cfg(target_os = "linux")]
#[log(skip_all)]
pub async fn handle_media_pause(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::pause().await {
        Ok(_) => {
            let msg = "Media playback paused successfully.";
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to pause media playback: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_play_pause(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::play_pause().await {
        Ok(_) => {
            let msg = "Media playback toggled successfully.";
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to toggle media playback: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_stop(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::stop().await {
        Ok(_) => {
            let msg = "Media playback stopped successfully.";
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to stop media playback: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_next_track(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::next_track().await {
        Ok(_) => {
            let msg = "Skipped to the next track successfully.";
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to skip to the next track: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_previous_track(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::previous_track().await {
        Ok(_) => {
            let msg = "Returned to the previous track successfully.";
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to return to the previous track: {e:?}").into()),
    }
}

#[log(skip_all, fields(action))]
pub async fn handle_media_seek_forward(tx: Sender<Bytes>, action: SeekAction) -> Result<()> {
    match MediaControl::seek_forward(action.seconds).await {
        Ok(_) => {
            let msg = str!(
                "Media playback advanced by {} seconds successfully.",
                action.seconds
            );
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to seek forward: {e:?}").into()),
    }
}

#[log(skip_all, fields(action))]
pub async fn handle_media_seek_backward(tx: Sender<Bytes>, action: SeekAction) -> Result<()> {
    match MediaControl::seek_backward(action.seconds).await {
        Ok(_) => {
            let msg = str!(
                "Media playback rewound by {} seconds successfully.",
                action.seconds
            );
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to seek backward: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_metadata(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::metadata().await {
        Ok(metadata) => {
            let msg = str!(metadata);

            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to retrieve media metadata: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_position(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::position().await {
        Ok(position) => {
            let msg = str!("Current playback position: {:?}.", position);
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to retrieve playback position: {e:?}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_media_duration(tx: Sender<Bytes>) -> Result<()> {
    match MediaControl::duration().await {
        Ok(duration) => {
            let msg = str!("Current media duration: {:?}.", duration);
            info!("{msg}");
            tx.send(Event::answer(msg))?;
            Ok(())
        }
        Err(e) => Err(str!("Failed to retrieve media duration: {e:?}").into()),
    }
}
