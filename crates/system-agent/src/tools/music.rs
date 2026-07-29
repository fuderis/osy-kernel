use crate::prelude::*;
use anylm::api::{Schema, Tool};
use music_index::{MusicIndexer, SearchIntent};

static MUSIC_INDEX: State<Option<MusicIndexer>> = State::default();

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //              SEARCH MUSIC
        Tool::new(
            "search_music",
            "Searches the local music library without starting playback.",
        )
        .optional_property(
            "query",
            Schema::string("General natural-language music search query."),
        )
        .optional_property("band", Schema::string("Artist or band name."))
        .optional_property("album", Schema::string("Album title."))
        .optional_property("track", Schema::string("Track title."))
        .optional_property("genre", Schema::string("Music genre.")),
        // ________________________________________
        //              PLAY MUSIC
        Tool::new(
            "play_music",
            "Searches the local music library and immediately starts playback.",
        )
        .optional_property(
            "query",
            Schema::string("General natural-language music search query."),
        )
        .optional_property("band", Schema::string("Artist or band name."))
        .optional_property("album", Schema::string("Album title."))
        .optional_property("track", Schema::string("Track title."))
        .optional_property("genre", Schema::string("Music genre.")),
    ]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MusicAction {
    pub query: Option<String>,
    pub band: Option<String>,
    pub album: Option<String>,
    pub track: Option<String>,
    pub genre: Option<String>,
}

async fn music_index() -> Result<MusicIndexer> {
    if MUSIC_INDEX.get().await.is_none() {
        let index = MusicIndexer::scan_default(path!("$cache$/music-index.json")).await?;
        MUSIC_INDEX.set(Some(index)).await;
    }

    MUSIC_INDEX
        .dirty_get()
        .as_ref()
        .clone()
        .ok_or_else(|| str!("Failed to initialize music indexer").into())
}

#[log(skip_all, fields(action))]
pub async fn handle_search_music(tx: Sender<Bytes>, mut action: MusicAction) -> Result<()> {
    let music_index = music_index().await?;

    let intent = if let Some(query) = action.query {
        SearchIntent::Global(query)
    } else if action.band.is_none()
        && action.album.is_none()
        && action.genre.is_none()
        && action.track.is_some()
    {
        SearchIntent::Global(action.track.take().unwrap())
    } else {
        SearchIntent::Targeted {
            band: action.band,
            album: action.album,
            track: action.track,
            genre: action.genre,
        }
    };

    let target = music_index.search(intent);
    let tracks = target.tracks();

    let msg = if tracks.is_empty() {
        str!("No matching music was found.")
    } else {
        str!("Found {count} matching track(s).", count = tracks.len())
    };

    info!("{msg}");
    tx.send(Event::answer(msg))?;

    Ok(())
}

#[log(skip_all, fields(action))]
pub async fn handle_play_music(tx: Sender<Bytes>, mut action: MusicAction) -> Result<()> {
    let music_index = music_index().await?;

    let intent = if let Some(query) = action.query {
        SearchIntent::Global(query)
    } else if action.band.is_none()
        && action.album.is_none()
        && action.genre.is_none()
        && action.track.is_some()
    {
        SearchIntent::Global(action.track.take().unwrap())
    } else {
        SearchIntent::Targeted {
            band: action.band,
            album: action.album,
            track: action.track,
            genre: action.genre,
        }
    };

    let target = music_index.search(intent);
    let tracks = target.tracks();

    if tracks.is_empty() {
        let msg = str!("No matching music was found.");
        info!("{msg}");
        tx.send(Event::answer(msg))?;
        return Ok(());
    }

    music_index
        .play(target, path!("/tmp/ovsy/playlist.m3u"))
        .await?;

    let msg = str!(
        "Started playback of {count} track(s).",
        count = tracks.len()
    );

    info!("{msg}");
    tx.send(Event::answer(msg))?;

    Ok(())
}
