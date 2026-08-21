use crate::{prelude::*, user::Session};

use anylm::{
    api::{Message, Messages},
    completions::{Chunk, Completions},
};
use osy_share::{CompactQuery, Event, RemoveQuery, SessionId, SessionInfo, SetQuery};

/// Initializes the user session and returns its messages
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_init(Paths(sid): Paths<SessionId>, data: Json<SessionInfo>) -> Response {
    let session_info = data.0;
    info!("Handling session init/get...");

    // Check active session, or initialize a new one
    let session_shared = match Session::get(&sid).await {
        Some(existing) => {
            info!("Found existing session in memory");
            existing
        }
        None => {
            info!("Session not found in memory, initializing new Session::init...");
            match Session::init(sid, session_info).await {
                Ok(s) => {
                    info!("Session::init succeeded");
                    s
                }
                Err(e) => {
                    error!("Failed to init session {sid}: {e}");
                    return Response::error().text(e.to_string());
                }
            }
        }
    };

    info!("Waiting for session_shared lock...");
    let read_result = {
        let session = session_shared.lock().await;
        info!("Acquired session_shared lock. Reading messages...");
        let res = session.read_messages().await;
        info!("Messages read. Releasing session_shared lock...");
        res
    };

    match read_result {
        Ok(messages) => {
            info!("Successfully finished handle_init");
            Response::ok().json(&messages)
        }
        Err(e) => {
            error!("Failed to read messages for session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Finishes the user session and flushes DB to prevent lock contention
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_finish(Paths(sid): Paths<SessionId>) -> Response {
    info!("Attempting Session::finish...");
    match Session::finish(&sid).await {
        Ok(_) => {
            info!("Session finished successfully");
            Response::ok().text("Session finished successfully")
        }
        Err(e) => {
            error!("Failed to finish session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// API: Handles the session compression
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_compact(Paths(sid): Paths<SessionId>, data: Json<CompactQuery>) -> Response {
    let CompactQuery { preserve } = data.0;
    let current = Span::current();

    Response::ok().stream(move |tx| {
        async move {
            let cfg = Settings::get();
            let preserve_count = preserve.unwrap_or(cfg.execution.preserve_messages);
            info!("Starting stream (preserve: {preserve_count})");

            info!("Looking up Session::get...");
            let Some(session_shared) = Session::get(&sid).await else {
                let err_msg = format!("Undefined session id `{sid}`");
                error!("{err_msg}");
                tx.send(Event::error(err_msg)).ok();
                return;
            };

            info!("Waiting for session_shared lock to read messages...");
            let db_messages = {
                let session = session_shared.lock().await;
                info!("Acquired session_shared lock. Reading messages...");
                let msgs_res = session.read_messages().await;
                info!("Messages read. Releasing session_shared lock...");

                match msgs_res {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        error!("Failed to read messages for compression: {e}");
                        tx.send(Event::error(e.to_string())).ok();
                        return;
                    }
                }
            };

            let compress_count = db_messages.len();
            info!("Total messages read: {compress_count}");
            if compress_count == 0 {
                warn!("Nothing to compress, skip");
                tx.send(Event::finish()).ok();
                return;
            }

            let mut messages = Messages::from(db_messages);
            let to_preserve: Vec<Message> = messages.slice(-(preserve_count as isize)).into();

            let messages = messages
                .user(vec![cfg.completions.compression_prompt.clone().into()])
                .wrap();

            let ops = cfg
                .compression
                .options
                .clone()
                .unwrap_or(cfg.completions.options.clone());

            info!("Sending compression request to LLM...");
            let mut response = match Completions::try_from(ops) {
                Ok(comp) => match comp.send(messages).await {
                    Ok(res) => {
                        info!("Received LLM stream response handle");
                        res
                    }
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
            info!("Streaming compressed response chunks from LLM...");

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

            info!(
                "LLM stream finished. Length of text: {}",
                full_compressed_text.len()
            );

            let compressed_message = Message::assistant(vec![full_compressed_text.into()], vec![]);

            info!("Waiting for session_shared lock to save compressed history...");
            let save_res = {
                let session = session_shared.lock().await;
                info!("Acquired session_shared lock. Inserting & shifting DB...");
                let res = session
                    .insert_and_shift(compressed_message, to_preserve, compress_count)
                    .await;
                info!("DB insert & shift complete. Releasing session_shared lock...");
                res
            };

            if let Err(e) = save_res {
                error!("Failed to update DB with compressed history: {e}");
                tx.send(Event::error(e.to_string())).ok();
                return;
            }

            tx.send(Event::finish()).ok();
            info!("Compression finished successfully for session {sid}");
        }
        .instrument(current)
    })
}

/// Completely clears the session message history
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_clear(Paths(sid): Paths<SessionId>) -> Response {
    info!("Requesting clear for session {sid}");

    info!("Looking up Session::get...");
    if let Some(session_shared) = Session::get(&sid).await {
        info!("Waiting for session_shared lock...");
        let res = {
            let session = session_shared.lock().await;
            info!("Acquired session_shared lock. Executing clear...");
            let clear_res = session.clear().await;
            info!("Clear complete. Releasing lock...");
            clear_res
        };

        if let Err(e) = res {
            error!("Failed to clear session {sid}: {e}");
            return Response::error().text(e.to_string());
        }
        info!("Successfully cleared session {sid}");
    } else {
        warn!("Attempted to clear non-existent session {sid}");
    }

    Response::ok()
}

/// Clones the user session and returns a new ID
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_clone(Paths(sid): Paths<SessionId>) -> Response {
    info!("Requesting clone for session {sid}");

    info!("Looking up Session::get...");
    if let Some(session_shared) = Session::get(&sid).await {
        info!("Waiting for session_shared lock...");
        let clone_res = {
            let session = session_shared.lock().await;
            info!("Acquired session_shared lock. Executing duplicate...");
            let res = session.duplicate().await;
            info!("Duplicate complete. Releasing lock...");
            res
        };

        match clone_res {
            Ok(new_sid) => {
                info!("Session cloned to `{new_sid}`");
                Response::ok().json(&json!({ "id": new_sid }))
            }
            Err(e) => {
                let msg = format!("Failed to clone session: {e}");
                error!("{msg}");
                Response::error().text(msg)
            }
        }
    } else {
        let msg = format!("Session `{sid}` is not defined");
        error!("{msg}");
        Response::error().text(msg)
    }
}

// --- LOCAL SESSION RULES HANDLERS ---

/// Lists active rules (global + local) for a session
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_rules_list(Paths(sid): Paths<SessionId>) -> Response {
    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session_shared lock...");
    let rules_res = {
        let session = session_shared.lock().await;
        info!("Acquired session_shared lock. Listing session rules...");
        let res = session.list_session_rules().await;
        info!("Rules listed. Releasing lock...");
        res
    };

    match rules_res {
        Ok(rules) => {
            info!("Finished listing rules for session {sid}");
            Response::ok().json(&rules)
        }
        Err(e) => {
            error!("Failed to list rules for session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Adds or updates a rule in the session or global context
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_rules_set(Paths(sid): Paths<SessionId>, data: Json<SetQuery>) -> Response {
    let SetQuery { id, text } = data.0;
    info!("Setting rule (id: {id:?})...");

    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session_shared lock...");
    let save_res = {
        let session = session_shared.lock().await;
        info!("Acquired session_shared lock...");

        if let Some(ref rule_id) = id {
            info!("Removing existing rule `{rule_id}` before overwrite...");
            if let Err(e) = session.remove_rule(*rule_id).await {
                warn!("Failed to remove existing rule `{rule_id}`: {e}");
            }
        }

        info!("Saving rule...");
        let res = session.save_rule(id, text, false).await;
        info!("Rule saved. Releasing lock...");
        res
    };

    match save_res {
        Ok(rule) => {
            info!("Rule set successfully");
            Response::ok().json(&rule)
        }
        Err(e) => {
            error!("Failed to set rule for session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Removes a rule from the active session context by ID
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_rules_remove(
    Paths(sid): Paths<SessionId>,
    data: Json<RemoveQuery>,
) -> Response {
    let rule_id = data.0.id;
    info!("Removing rule `{rule_id}`...");

    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session_shared lock...");
    let remove_res = {
        let session = session_shared.lock().await;
        info!("Acquired session_shared lock. Removing rule `{rule_id}`...");
        let res = session.remove_rule(rule_id.clone()).await;
        info!("Remove operation finished. Releasing lock...");
        res
    };

    match remove_res {
        Ok(deleted) => {
            if deleted {
                info!("Rule `{rule_id}` removed successfully");
                Response::ok().text("Rule removed successfully")
            } else {
                warn!("Rule `{rule_id}` not found");
                Response::error().text(format!("Rule `{rule_id}` not found"))
            }
        }
        Err(e) => {
            error!("Failed to remove rule `{rule_id}` for session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// Clears only the local rules for a session
#[log(skip_all, fields(sid = %sid))]
pub async fn handle_rules_clear(Paths(sid): Paths<SessionId>) -> Response {
    info!("Requesting clear_local_rules...");

    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session_shared lock...");
    let clear_res = {
        let session = session_shared.lock().await;
        info!("Acquired session_shared lock. Clearing local rules...");
        let res = session.clear_local_rules().await;
        info!("Local rules cleared. Releasing lock...");
        res
    };

    match clear_res {
        Ok(_) => {
            info!("Local session rules cleared successfully");
            Response::ok().text("Local session rules cleared successfully")
        }
        Err(e) => {
            error!("Failed to clear local rules for session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}
