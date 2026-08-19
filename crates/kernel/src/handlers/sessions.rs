use crate::{prelude::*, session::Session};

use anylm::{
    api::{Message, Messages},
    completions::{Chunk, Completions},
};
use osy_share::{CompactQuery, Event, RemoveQuery, SessionId, SessionInfo, SetQuery};

/// Initializes the user session and returns its messages
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_init(sid: Paths<SessionId>, data: Json<SessionInfo>) -> Response {
    let session_id = sid.0;
    let session_info = data.0;

    // Check active session, or initialize a new one
    let session_shared = if let Some(existing) = Session::get(&session_id) {
        existing
    } else {
        match Session::init(session_id, session_info).await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to init session {session_id}: {e}");
                return Response::error().text(e.to_string());
            }
        }
    };

    // Lock session and read history
    let session = session_shared.lock().await;
    match session.read_messages().await {
        Ok(messages) => Response::ok().json(&messages),
        Err(e) => {
            error!("Failed to read messages for session {session_id}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Finishes the user session and flushes DB to prevent lock contention
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_finish(sid: Paths<SessionId>) -> Response {
    let session_id = sid.0;

    match Session::finish(&session_id).await {
        Ok(_) => Response::ok().text("Session finished successfully"),
        Err(e) => {
            error!("Failed to finish session {session_id}: {e}");
            Response::error().text(e.to_string())
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

            // Get session from active registry
            let Some(session_shared) = Session::get(&session_id) else {
                let err_msg = format!("Undefined session id `{session_id}`");
                error!("{err_msg}");
                tx.send(Event::error(err_msg)).ok();
                return;
            };

            // Read existing messages under brief lock scoping
            let db_messages = {
                let session = session_shared.lock().await;
                match session.read_messages().await {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        error!("Failed to read messages for compression: {e}");
                        tx.send(Event::error(e.to_string())).ok();
                        return;
                    }
                }
            };

            let compress_count = db_messages.len();
            if compress_count == 0 {
                warn!("Nothing to compress, skip");
                tx.send(Event::finish()).ok();
                return;
            }

            let mut messages = Messages::from(db_messages);

            // Select messages to preserve
            let to_preserve: Vec<Message> = messages.slice(-(preserve_count as isize)).into();

            // Prepare history compression prompt
            let messages = messages
                .user(vec![cfg.completions.compression_prompt.clone().into()])
                .wrap();

            // Send request to LLM (without holding session Mutex lock!)
            let ops = cfg
                .compression
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

            // Stream response to client and aggregate compressed content
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

            // Write updated message history back into database
            let compressed_message = Message::assistant(vec![full_compressed_text.into()], vec![]);
            let session = session_shared.lock().await;
            if let Err(e) = session
                .insert_and_shift(compressed_message, to_preserve, compress_count)
                .await
            {
                error!("Failed to update DB with compressed history: {e}");
                tx.send(Event::error(e.to_string())).ok();
                return;
            }

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
        let session = session_shared.lock().await;
        if let Err(e) = session.clear().await {
            error!("Failed to clear session {session_id}: {e}");
            return Response::error().text(e.to_string());
        }
    } else {
        warn!("Attempted to clear non-existent session {session_id}");
    }

    Response::ok()
}

/// Clones the user session and returns a new ID
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_clone(Paths(sid): Paths<SessionId>) -> Response {
    if let Some(session) = Session::get(&sid) {
        match session.lock().await.duplicate().await {
            Ok(new_sid) => {
                info!("Session cloned to `{new_sid}`");
                Response::ok().json(&json! ({"id": new_sid}))
            }

            Err(e) => {
                let msg = str!("Failed to clone session: {e}");
                error!("{msg}");
                Response::error().text(msg.to_string())
            }
        }
    } else {
        let msg = str!("Session `{sid}` is not defined");
        error!("{msg}");
        Response::error().text(msg.to_string())
    }
}

// --- LOCAL SESSION RULES HANDLERS ---

/// Lists active rules (global + local) for a session
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_rules_list(sid: Paths<SessionId>) -> Response {
    let session_id = sid.0;

    let Some(session_shared) = Session::get(&session_id) else {
        let err_msg = format!("Undefined session id `{session_id}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    let session = session_shared.lock().await;
    match session.list_session_rules().await {
        Ok(rules) => Response::ok().json(&rules),
        Err(e) => {
            error!("Failed to list rules for session {session_id}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Adds or updates a rule in the session or global context
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_rules_set(sid: Paths<SessionId>, data: Json<SetQuery>) -> Response {
    let session_id = sid.0;
    let SetQuery { id, text } = data.0;

    let Some(session_shared) = Session::get(&session_id) else {
        let err_msg = format!("Undefined session id `{session_id}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    let session = session_shared.lock().await;

    // Если был передан id, удаляем старое правило перед перезаписью
    if let Some(ref rule_id) = id {
        if let Err(e) = session.remove_rule(*rule_id).await {
            warn!("Failed to remove existing rule `{rule_id}` before overwrite: {e}");
        }
    }

    match session.save_rule(id, text, false).await {
        Ok(rule) => Response::ok().json(&rule),
        Err(e) => {
            error!("Failed to set rule for session {session_id}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Removes a rule from the active session context by ID
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_rules_remove(sid: Paths<SessionId>, data: Json<RemoveQuery>) -> Response {
    let session_id = sid.0;
    let rule_id = data.0.id;

    let Some(session_shared) = Session::get(&session_id) else {
        let err_msg = format!("Undefined session id `{session_id}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    let session = session_shared.lock().await;
    match session.remove_rule(rule_id.clone()).await {
        Ok(deleted) => {
            if deleted {
                Response::ok().text("Rule removed successfully")
            } else {
                Response::error().text(format!("Rule `{rule_id}` not found"))
            }
        }
        Err(e) => {
            error!("Failed to remove rule `{rule_id}` for session {session_id}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Clears only the local rules for a session
#[log(skip_all, fields(sid = %sid.0))]
pub async fn handle_rules_clear(sid: Paths<SessionId>) -> Response {
    let session_id = sid.0;

    let Some(session_shared) = Session::get(&session_id) else {
        let err_msg = format!("Undefined session id `{session_id}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    let session = session_shared.lock().await;
    match session.clear_local_rules().await {
        Ok(_) => Response::ok().text("Local session rules cleared successfully"),
        Err(e) => {
            error!("Failed to clear local rules for session {session_id}: {e}");
            Response::error().text(e.to_string())
        }
    }
}
