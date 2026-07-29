use crate::{prelude::*, session::Session};

use anylm::{
    api::{Message, Messages},
    completions::{Chunk, Completions},
};
use ovsy_share::{CompactQuery, Event, SessionId, SessionInfo};

/// Initializes the user session and returns its messages
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_init(sid: Paths<SessionId>, data: Json<SessionInfo>) -> Response {
    let session_id = sid.0;
    let session_info = data.0;

    // check active session, or initialize a new one
    let session_shared = if let Some(existing) = Session::get(&session_id) {
        existing
    } else {
        match Session::init(session_id, session_info).await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to init session {session_id}: {e}");
                return Response::bad_request().text(e.to_string());
            }
        }
    };

    // block mutex and read messages from the database
    let lock = session_shared.lock().await;
    match lock.read_messages().await {
        Ok(messages) => Response::ok().json(&messages),
        Err(e) => {
            error!("Failed to read messages for session {session_id}: {e}");
            Response::bad_request().text(e.to_string())
        }
    }
}

/// Finishes the user session and flushes DB to prevent sled locks
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_finish(sid: Paths<SessionId>) -> Response {
    let session_id = sid.0;

    match Session::finish(&session_id).await {
        Ok(_) => Response::ok().text("Session finished successfully"),
        Err(e) => {
            error!("Failed to finish session {session_id}: {e}");
            Response::bad_request().text(e.to_string())
        }
    }
}

/// API: Handles the session compression
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_compact(sid: Paths<SessionId>, data: Json<CompactQuery>) -> Response {
    let session_id = sid.0;
    let CompactQuery { preserve } = data.0;
    let current = Span::current();

    Response::ok().stream(move |tx| {
        async move {
            let cfg = Settings::get();
            let preserve_count = preserve.unwrap_or(cfg.execution.preserve_messages);
            info!("Compressing session messages (preserve: {preserve_count})");

            let compression_cfg = cfg.compression.clone();

            // get session from the global state
            let Some(session_shared) = Session::get(&session_id) else {
                let err_msg = format!("Undefined session id `{session_id}`");
                error!("{err_msg}");
                tx.send(Event::error(err_msg)).ok();
                return;
            };

            // read the messages after logging into the session
            let db_messages = match session_shared.lock().await.read_messages().await {
                Ok(msgs) => msgs,
                Err(e) => {
                    error!("Failed to read messages for compression: {e}");
                    tx.send(Event::error(e.to_string())).ok();
                    return;
                }
            };

            let compress_count = db_messages.len();
            if compress_count == 0 {
                warn!("Nothing to compress, skip");
                tx.send(Event::finish()).ok();
                return;
            }

            let mut messages = Messages::from(db_messages);

            // select the messages that need to be left untouched
            let to_preserve: Vec<Message> = messages.slice(-(preserve_count as isize)).into();

            // creating a prompt to compress the history
            let messages = messages.user(vec![compression_cfg.prompt.into()]).wrap();

            // sending a request to the LLM
            let ops = compression_cfg
                .options
                .clone()
                .unwrap_or(cfg.completions.options.clone());
            let mut response = match Completions::try_from(ops) {
                Ok(comp) => match comp.send(messages).await {
                    Ok(res) => res,
                    Err(e) => {
                        error!("Failed to send compression request to LLM: {e}");
                        tx.send(Event::error(e.to_string())).ok();
                        return;
                    }
                },
                Err(e) => {
                    error!("Failed to prepare LLM completions config: {e}");
                    tx.send(Event::error(e.to_string())).ok();
                    return;
                }
            };

            let mut full_compressed_text = String::new();

            // stream the response to the user and collect the full text
            while let Some(chunk) = response.next().await {
                match chunk {
                    Ok(Chunk::Text(text_part)) => {
                        if tx.send(Event::answer(text_part.clone())).is_err() {
                            warn!("Stream receiver dropped by client, aborting compression");
                            return;
                        }
                        full_compressed_text.push_str(&text_part);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error during LLM streaming: {e}");
                        tx.send(Event::error(e.to_string())).ok();
                        return;
                    }
                }
            }

            // rewriting the history in the database: put the compressed version and shift the saved messages
            let compressed_message = Message::assistant(vec![full_compressed_text.into()], vec![]);
            if let Err(e) = session_shared
                .lock()
                .await
                .insert_and_shift(compressed_message, to_preserve, compress_count)
                .await
            {
                error!("Failed to update DB with compressed history: {e}");
                tx.send(Event::error(e.to_string())).ok();
                return;
            }

            // successful finish the stream
            tx.send(Event::finish()).ok();
            info!("Compression finished successfully for session {session_id}");
        }
        .instrument(current)
    })
}

/// Completely clears the session message history
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_clear(sid: Paths<SessionId>) -> Response {
    let session_id = sid.0;
    info!("Clearing history for session: {session_id}");

    if let Some(session_shared) = Session::get(&session_id) {
        if let Err(e) = session_shared.lock().await.clear().await {
            error!("Failed to clear session {session_id}: {e}");
            return Response::bad_request().text(e.to_string());
        }
    } else {
        warn!("Attempted to clear non-existent session {session_id}");
    }

    Response::ok()
}
