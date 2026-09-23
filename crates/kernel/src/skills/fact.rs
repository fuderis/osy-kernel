use crate::{prelude::*, user::UserState};

use anylm::api::{Schema, Tool};
use osy_share::{SearchQuery, SetQuery};

/// Returns tools list.
pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new(
            "remember_fact",
            "Saves a new persistent fact or memory about the user into long-term memory. \
            Use this when the user explicitly asks to remember something or discloses important user-specific information \
            (e.g., preferences, personal facts, project settings, tech stack).",
        )
        .required_property(
            "text",
            Schema::string("The clear, natural text description of the fact to store."),
        ),

        Tool::new(
            "search_fact",
            "Searches long-term user memory using semantic and keyword search. \
            Returns matching facts with their IDs, relevance, and usage info.",
        )
        .required_property(
            "query",
            Schema::string(
                "A search query. Make a short, keyword-rich query in English, focused on the facts the user is interested in."
            ),
        )
        .optional_property(
            "limit",
            Schema::string(
                "A limit on number of facts that must be found."
            ),
        )
    ]
}

// --- Action Payload Deserializers ---

/// Saves new fact to the user's vector storage.
#[log(uid = %user.id)]
pub async fn handle_remember_fact(user: &UserState, data: SetQuery) -> Result<String> {
    let SetQuery { text, .. } = data;

    // save fact to database
    if let Err(e) = user.save_fact(text.clone()).await {
        error!("Failed to save user fact: {e}");
        return Err(e);
    };

    info!("Saved new user fact: `{text}`");
    Ok(format!("Fact successfully saved: \"{text}\""))
}

/// Searches for relevant user facts.
#[log(uid = %user.id)]
pub async fn handle_search_fact(user: &UserState, data: SearchQuery) -> Result<String> {
    let SearchQuery { query, limit } = data;

    // search facts in database
    let records = match user.search_facts(&query, limit).await {
        Ok(res) => res,
        Err(e) => {
            error!("Failed to search user's facts: {e}");
            return Err(e);
        }
    };

    // check records len
    if records.is_empty() {
        info!("Search for fact `{query}` returned no results.");
        return Ok(format!("No relevant facts found for query: \"{query}\""));
    }
    info!("Found {} facts for query `{query}`", records.len());

    // collect response
    let mut response = format!(
        "Found {} relevant facts for query \"{query}\":\n",
        records.len()
    );
    for record in records {
        response.push_str(&format!("* [ID: {}] {}\n", record.id, record.data.text));
    }

    Ok(response)
}
