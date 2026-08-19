use crate::{context, prelude::*, session::UserState};
use anylm::embeddings::EmbeddingSearch;
use osy_share::{ListQuery, RemoveQuery, SearchQuery, SetQuery};

/// Handles the user sessions list
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_list(uid: Paths<u128>, data: Json<ListQuery>) -> Response {
    let count = data.0.count.unwrap_or(0);

    match UserState::sessions_list(*uid, count).await {
        Ok(sessions) => Response::ok().json(&sessions),
        Err(e) => {
            error!("Failed to fetch user sessions for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Lists all user facts stored in RAG memory
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_facts_list(uid: Paths<u128>, data: Json<ListQuery>) -> Response {
    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    match user_db.list_all_facts().await {
        Ok(mut facts) => Response::ok().json({
            if let Some(count) = data.0.count {
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

// TODO: /// Vector search across user facts
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_facts_search(uid: Paths<u128>, _data: Json<SearchQuery>) -> Response {
    Response::error().text("This endpoint is not implemented yet, sorry =(..")

    // let query = data.0;
    // let cfg = Settings::get();

    // let limit = query.limit.unwrap_or(cfg.context.search_limit);
    // let threshold = query.threshold.unwrap_or(cfg.context.fact_similarity);

    // let user_db = match UserState::get_or_init(*uid).await {
    //     Ok(db) => db,
    //     Err(e) => return Response::error().text(e.to_string()),
    // };

    // match user_db
    //     .search_facts(query.embedding, limit, threshold)
    //     .await
    // {
    //     Ok(results) => Response::ok().json(&results),
    //     Err(e) => {
    //         error!("Failed to search facts for user {}: {e}", *uid);
    //         Response::error().text(e.to_string())
    //     }
    // }
}

/// Adds or updates a user fact in RAG memory
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_facts_set(uid: Paths<u128>, data: Json<SetQuery>) -> Response {
    let SetQuery { id, text } = data.0;

    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    // if the fact ID was passed, first delete the old one.
    if let Some(ref fact_id) = id {
        if let Err(e) = user_db.remove_fact(*fact_id).await {
            error!(
                "Failed to remove existing fact {fact_id} before overwrite for user {}: {e}",
                *uid
            );
        }
    }

    // 1. Нормализуем исходный текст факта
    let search_text = context::normalize_fact_text(&text).await;

    // 2. Генерируем эмбеддинг по нормализованному тексту
    let embedding = match context::generate_embedding(&search_text, EmbeddingSearch::Document).await
    {
        Ok(emb) => emb,
        Err(e) => {
            error!("Failed to generate embedding for user {}: {e}", *uid);
            return Response::error().text(e.to_string());
        }
    };

    // 3. Сохраняем факт вместе с нормализованным текстом в DB
    match user_db
        .save_fact(embedding, text, Some(search_text.into()))
        .await
    {
        Ok(_) => Response::ok().text("Fact saved successfully"),
        Err(e) => {
            error!("Failed to set fact for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Removes a single fact by its ID
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_facts_remove(uid: Paths<u128>, data: Json<RemoveQuery>) -> Response {
    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    match user_db.remove_fact(data.id).await {
        Ok(_) => Response::ok().text("Fact removed successfully"),
        Err(e) => {
            error!("Failed to remove fact {} for user {}: {e}", data.id, *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Clears all user facts from RAG database
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_facts_clear(uid: Paths<u128>) -> Response {
    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    match user_db.clear_all_facts().await {
        Ok(_) => Response::ok().text("All facts cleared successfully"),
        Err(e) => {
            error!("Failed to clear facts for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Lists global rules for the specified user
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_rules_list(uid: Paths<u128>, data: Json<ListQuery>) -> Response {
    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    match user_db.list_global_rules().await {
        Ok(mut rules) => Response::ok().json({
            if let Some(count) = data.count {
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

/// Adds or updates a global user rule
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_rules_set(uid: Paths<u128>, data: Json<SetQuery>) -> Response {
    let SetQuery { id, text } = data.0;

    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    // save the rule as global (`is_global = true`)
    match user_db.save_global_rule(id, text).await {
        Ok(rule) => Response::ok().json(&rule),
        Err(e) => {
            error!("Failed to set global rule for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Removes a global user rule by ID
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_rules_remove(uid: Paths<u128>, data: Json<RemoveQuery>) -> Response {
    let rule_id = data.id;
    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    match user_db.remove_global_rule(rule_id.clone()).await {
        Ok(true) => Response::ok().text("Global rule removed successfully"),
        Ok(false) => Response::error().text(format!("Rule `{rule_id}` not found")),
        Err(e) => {
            error!("Failed to remove rule `{rule_id}` for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}

/// Clears all global rules for the user
#[log(skip_all, fields(uid = %*uid))]
pub async fn handle_rules_clear(uid: Paths<u128>) -> Response {
    let user_db = match UserState::get_or_init(*uid).await {
        Ok(db) => db,
        Err(e) => return Response::error().text(e.to_string()),
    };

    match user_db.clear_global_rules().await {
        Ok(_) => Response::ok().text("Global user rules cleared successfully"),
        Err(e) => {
            error!("Failed to clear global rules for user {}: {e}", *uid);
            Response::error().text(e.to_string())
        }
    }
}
