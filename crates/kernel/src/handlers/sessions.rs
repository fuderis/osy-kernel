use crate::{prelude::*, user::Session};

use anylm::{
    api::{Message, Messages},
    completions::{Chunk, Completions},
};
use osy_share::{CompactQuery, Event, RemoveQuery, SessionId, SessionInfo, SetQuery};

/// API: Initializes the user session and returns its messages
#[log(sid = %sid)]
pub async fn handle_session_init(
    Paths(sid): Paths<SessionId>,
    payload: Json<SessionInfo>,
) -> Response {
    let session_info = payload.0;
    info!("Handling session init/get...");

    // check active session, or initialize a new one
    let session_shared = match Session::get(&sid).await {
        Some(existing) => {
            info!("Found existing session in memory");
            existing
        }
        None => match Session::init(sid, session_info).await {
            Ok(s) => s,
            Err(e) => {
                error!("Failed to init session {sid}: {e}");
                return Response::error().text(e.to_string());
            }
        },
    };

    info!("Waiting for session lock...");
    let read_result = {
        let session = session_shared.lock().await;
        let res = session.read_messages().await;
        info!("Messages read. Releasing session lock...");
        res
    };

    match read_result {
        Ok(messages) => Response::ok().json(&messages),
        Err(e) => {
            error!("Failed to read messages for session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// API: Finishes the user session and flushes DB to prevent lock contention
#[log(sid = %sid)]
pub async fn handle_session_finish(Paths(sid): Paths<SessionId>) -> Response {
    match Session::finish(&sid).await {
        Ok(_) => Response::ok().text("Session finished successfully"),
        Err(e) => {
            error!("Failed to finish session {sid}: {e}");
            Response::error().text(e.to_string())
        }
    }
}

/// API: Handles the session compression
#[log(sid = %sid)]
pub async fn handle_session_compact(
    Paths(sid): Paths<SessionId>,
    payload: Json<CompactQuery>,
) -> Response {
    let CompactQuery { preserve } = payload.0;
    let current = Span::current();

    Response::ok().stream(move |tx| {
        async move {
            let cfg = Config::get();
            let preserve_count = preserve.unwrap_or(cfg.execution.preserve_messages);
            let provider_options = cfg
                .completions
                .options
                .clone()
                .temperature(cfg.completions.compress_temp);
            let compress_prompt = cfg.prompts.compress_prompt.clone();

            info!("Starting stream (preserve: {preserve_count})");

            let Some(session_shared) = Session::get(&sid).await else {
                let err_msg = format!("Undefined session id `{sid}`");
                error!("{err_msg}");
                tx.send(Event::Error(err_msg)).ok();
                return;
            };

            info!("Waiting for session lock to read messages...");
            let db_messages = {
                let session = session_shared.lock().await;
                let msgs_res = session.read_messages().await;
                info!("Messages read. Releasing session lock...");

                match msgs_res {
                    Ok(msgs) => msgs,
                    Err(e) => {
                        error!("Failed to read messages for compression: {e}");
                        tx.send(Event::Error(e.to_string())).ok();
                        return;
                    }
                }
            };

            let compress_count = db_messages.len();
            info!("Total messages read: {compress_count}");
            if compress_count == 0 {
                warn!("Nothing to compress, skip");
                tx.send(Event::Finish).ok();
                return;
            }

            let mut messages = Messages::from(db_messages);
            let to_preserve: Vec<Message> = messages.slice(-(preserve_count as isize)).into();

            let messages = messages.user(vec![compress_prompt.into()]).wrap();

            info!("Sending compression request to LLM...");
            let mut response = match Completions::try_from(provider_options) {
                Ok(comp) => match comp.send(messages).await {
                    Ok(res) => {
                        info!("Received LLM stream response handle");
                        res
                    }
                    Err(e) => {
                        error!("Failed to send compression request to LLM: {e}");
                        tx.send(Event::Error(e.to_string())).ok();
                        return;
                    }
                },
                Err(e) => {
                    error!("Failed to prepare LLM completions config: {e}");
                    tx.send(Event::Error(e.to_string())).ok();
                    return;
                }
            };

            let mut full_compressed_text = String::new();
            info!("Streaming compressed response chunks from LLM...");

            while let Some(chunk) = response.next().await {
                match chunk {
                    Ok(Chunk::Text(text_part)) => {
                        if tx.send(Event::Answer(text_part.clone())).is_err() {
                            warn!("Stream receiver dropped by client, aborting compression");
                            return;
                        }
                        full_compressed_text.push_str(&text_part);
                    }
                    Ok(_) => {}
                    Err(e) => {
                        error!("Error during LLM streaming: {e}");
                        tx.send(Event::Error(e.to_string())).ok();
                        return;
                    }
                }
            }

            info!(
                "LLM stream finished. Length of text: {}",
                full_compressed_text.len()
            );

            let compressed_message = Message::assistant(vec![full_compressed_text.into()], vec![]);

            info!("Waiting for session lock to save compressed history...");
            let save_res = {
                let session = session_shared.lock().await;
                info!("Acquired session lock. Inserting & shifting DB...");
                let res = session
                    .insert_and_shift(compressed_message, to_preserve, compress_count)
                    .await;
                info!("DB insert & shift complete. Releasing session lock...");
                res
            };

            if let Err(e) = save_res {
                error!("Failed to update DB with compressed history: {e}");
                tx.send(Event::Error(e.to_string())).ok();
                return;
            }

            tx.send(Event::Finish).ok();
            info!("Compression finished successfully for session {sid}");
        }
        .instrument(current)
    })
}

/// Completely clears the session message history
#[log(sid = %sid)]
pub async fn handle_session_clear(Paths(sid): Paths<SessionId>) -> Response {
    if let Some(session_shared) = Session::get(&sid).await {
        info!("Waiting for session lock...");
        let res = {
            let session = session_shared.lock().await;
            let clear_res = session.clear().await;
            clear_res
        };

        if let Err(e) = res {
            error!("Failed to clear session {sid}: {e}");
            return Response::error().text(e.to_string());
        }
    } else {
        warn!("Attempted to clear non-existent session {sid}.");
    }

    Response::ok()
}

/// Clones the user session and returns a new ID
#[log(sid = %sid)]
pub async fn handle_session_clone(Paths(sid): Paths<SessionId>) -> Response {
    if let Some(session_shared) = Session::get(&sid).await {
        info!("Waiting for session lock...");
        let clone_res = {
            let session = session_shared.lock().await;
            let res = session.duplicate().await;
            info!("Duplicate complete. Releasing lock...");
            res
        };

        match clone_res {
            Ok(new_sid) => Response::ok().json(&json!({ "id": new_sid })),
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

/// API: Lists active rules (global + local) for a session
#[log(sid = %sid)]
pub async fn handle_session_rules_list(Paths(sid): Paths<SessionId>) -> Response {
    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session lock...");
    let rules_res = {
        let session = session_shared.lock().await;
        info!("Acquired session lock. Listing session rules...");
        let res = session.load_rules().await;
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

/// API: Adds or updates a rule in the session or global context
#[log(sid = %sid)]
pub async fn handle_session_rules_set(
    Paths(sid): Paths<SessionId>,
    payload: Json<SetQuery>,
) -> Response {
    let SetQuery { id, text } = payload.0;
    info!("Setting rule (id: {id:?})...");

    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session lock...");
    let save_res = {
        let session = session_shared.lock().await;
        info!("Acquired session lock...");

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

/// API: Removes a rule from the active session context by ID
#[log(sid = %sid)]
pub async fn handle_session_rules_remove(
    Paths(sid): Paths<SessionId>,
    payload: Json<RemoveQuery>,
) -> Response {
    let rule_id = payload.0.id;
    info!("Removing rule `{rule_id}`...");

    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session lock...");
    let remove_res = {
        let session = session_shared.lock().await;
        info!("Acquired session lock. Removing rule `{rule_id}`...");
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

/// API: Clears only the local rules for a session
#[log(sid = %sid)]
pub async fn handle_session_rules_clear(Paths(sid): Paths<SessionId>) -> Response {
    info!("Requesting clear_local_rules...");

    info!("Looking up Session::get...");
    let Some(session_shared) = Session::get(&sid).await else {
        let err_msg = format!("Undefined session id `{sid}`");
        error!("{err_msg}");
        return Response::error().text(err_msg);
    };

    info!("Waiting for session lock...");
    let clear_res = {
        let session = session_shared.lock().await;
        info!("Acquired session lock. Clearing local rules...");
        let res = session.clear_rules().await;
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
