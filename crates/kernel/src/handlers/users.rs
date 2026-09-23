//! Users management handlers.

use crate::{prelude::*, user::UserState};

use osy_share::{ListQuery, RemoveQuery, SearchQuery, SetQuery};

macro_rules! get_user {
    ($uid:expr) => {{
        let user = match UserState::get_or_init($uid).await {
            Ok(user) => user,
            Err(e) => return Response::error().text(e.to_string()),
        };
        user
    }};
}

/// API: Handles user sessions list.
#[log(uid = %*uid)]
pub async fn handle_user_sessions_list(uid: Paths<u64>, payload: Json<ListQuery>) -> Response {
    let count = payload.0.count.unwrap_or(0);

    match UserState::list_sessions(*uid, count).await {
        Ok(sessions) => Response::ok().json(&sessions),
        Err(e) => {
            error!("Failed to fetch user sessions for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Lists all user facts stored in RAG memory.
#[log(uid = %*uid)]
pub async fn handle_user_facts_list(uid: Paths<u64>, payload: Json<ListQuery>) -> Response {
    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.load_facts().await {
        Ok(mut facts) => Response::ok().json({
            if let Some(count) = payload.0.count {
                facts.truncate(count);
            };
            &facts
        }),
        Err(e) => {
            error!("Failed to list facts for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Vector search across user facts.
#[log(uid = %*uid)]
pub async fn handle_user_facts_search(uid: Paths<u64>, payload: Json<SearchQuery>) -> Response {
    let SearchQuery { query, limit } = payload.0;

    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.search_facts(&query, limit).await {
        Ok(records) => {
            let facts: Vec<_> = records.into_iter().map(|record| record.data).collect();
            Response::ok().json(&facts)
        }
        Err(e) => {
            error!("Failed to search facts for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Adds or updates user fact in RAG memory.
#[log(uid = %*uid)]
pub async fn handle_user_facts_set(uid: Paths<u64>, payload: Json<SetQuery>) -> Response {
    let SetQuery { id, text } = payload.0;

    let user = get_user!(*uid);
    let user_guard = user.read().await;

    // if fact ID was passed, first delete the old one
    if let Some(ref fact_id) = id {
        if let Err(e) = user_guard.remove_fact(*fact_id).await {
            error!(
                "Failed to remove existing fact {fact_id} before overwrite for user {}: {e}",
                *uid
            );
        }
    }

    // save fact in DB
    match user_guard.save_fact(text).await {
        Ok(_) => Response::ok().text("Fact saved successfully"),
        Err(e) => {
            error!("Failed to set fact for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Removes fact by its ID.
#[log(uid = %*uid)]
pub async fn handle_user_facts_remove(uid: Paths<u64>, payload: Json<RemoveQuery>) -> Response {
    let RemoveQuery { id } = payload.0;

    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.remove_fact(id).await {
        Ok(_) => Response::ok().text("Fact removed successfully"),
        Err(e) => {
            error!("Failed to remove fact {id} for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Clears all user's facts.
#[log(uid = %*uid)]
pub async fn handle_user_facts_clear(uid: Paths<u64>) -> Response {
    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.clear_facts().await {
        Ok(_) => Response::ok().text("All facts cleared successfully"),
        Err(e) => {
            error!("Failed to clear facts for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Lists global rules for the specified user.
#[log(uid = %*uid)]
pub async fn handle_user_rules_list(uid: Paths<u64>, payload: Json<ListQuery>) -> Response {
    let ListQuery { count } = payload.0;

    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.load_rules().await {
        Ok(mut rules) => Response::ok().json({
            if let Some(count) = count {
                rules.truncate(count);
            };
            &rules
        }),
        Err(e) => {
            error!("Failed to list global rules for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Adds or updates a global user rule.
#[log(uid = %*uid)]
pub async fn handle_user_rules_set(uid: Paths<u64>, payload: Json<SetQuery>) -> Response {
    let SetQuery { id, text } = payload.0;

    let user = get_user!(*uid);
    let user_guard = user.read().await;

    // save the rule as global (`is_global = true`)
    match user_guard.save_rule(id, text).await {
        Ok(rule) => Response::ok().json(&rule),
        Err(e) => {
            error!("Failed to set global rule for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Removes a global user rule by ID.
#[log(uid = %*uid)]
pub async fn handle_user_rules_remove(uid: Paths<u64>, payload: Json<RemoveQuery>) -> Response {
    let RemoveQuery { id } = payload.0;

    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.remove_rule(id).await {
        Ok(true) => Response::ok().text("Global rule removed successfully"),
        Ok(false) => Response::error().text(format!("Rule `{id}` not found")),
        Err(e) => {
            error!("Failed to remove rule `{id}` for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// API: Clears all global rules for the user
#[log(uid = %*uid)]
pub async fn handle_user_rules_clear(uid: Paths<u64>) -> Response {
    let user = get_user!(*uid);
    let user_guard = user.read().await;

    match user_guard.clear_rules().await {
        Ok(_) => Response::ok().text("Global user rules cleared successfully"),
        Err(e) => {
            error!("Failed to clear global rules for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}
