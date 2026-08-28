use crate::{context, helpers, prelude::*, user::Session};
use anylm::{
    api::{Schema, Tool},
    embeddings::EmbeddingSearch,
};

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
            "search_text",
            Schema::string(
                "A search query. Make a short, keyword-rich query in English, focused on the facts the user is interested in."
            ),
        )
        .optional_property(
            "start_date",
            Schema::string(
                "Optional ISO 8601 / RFC 3339 UTC timestamp representing the start of the search range \
                (e.g. '2026-05-01T00:00:00Z'). Calculate based on the current UTC date if user asks for relative time like 'last week'.",
            ),
        )
        .optional_property(
            "end_date",
            Schema::string(
                "Optional ISO 8601 / RFC 3339 UTC timestamp representing the end of the search range \
                (e.g. '2026-05-07T23:59:59Z'). Calculate based on current UTC time.",
            ),
        ),
    ]
}

// --- Action Payload Deserializers ---

#[derive(Deserialize, Debug)]
pub struct RememberFactAction {
    pub text: String,
}

#[derive(Deserialize, Debug)]
pub struct ForgetFactAction {
    pub fact_id: u64,
}

#[derive(Deserialize, Debug)]
pub struct SearchFactAction {
    pub search_text: String,
    pub start_date: Option<String>,
    pub end_date: Option<String>,
}

/// Saves a new fact to the user's vector storage with automatic embedding generation
pub async fn handle_remember_fact(session: &Session, action: RememberFactAction) -> Result<String> {
    let fact_text = action.text.trim().to_string();
    if fact_text.is_empty() {
        return Ok("Fact text cannot be empty.".into());
    }

    // normalize the text using a separate function.
    let search_text = context::normalize_fact_text(&fact_text).await;

    // generate an embedding based on the normalized text.
    let embedding = context::generate_embedding(&search_text, EmbeddingSearch::Document).await?;

    // saving the fact to the database
    session
        .save_fact(embedding, fact_text.clone(), Some(search_text.into()))
        .await?;

    info!("Saved new user fact: '{fact_text}'");
    Ok(format!("Fact successfully saved: \"{fact_text}\""))
}

/// Searches for relevant user facts using semantic vector search
pub async fn handle_search_fact(session: &Session, action: SearchFactAction) -> Result<String> {
    let raw_query = action.search_text.trim();
    if raw_query.is_empty() {
        return Ok("Search query is empty.".into());
    }

    // if it’s not English, translate it using LLM.
    let query_text = if !helpers::is_english(raw_query) {
        context::translate_into_english(raw_query)
            .await
            .unwrap_or(raw_query.to_string())
    } else {
        raw_query.to_string()
    };

    // generate an embedding based on strictly English text.
    let embedding = context::generate_embedding(&query_text, EmbeddingSearch::Query).await?;

    let settings = Settings::get();
    let limit = settings.context.search_limit;
    let distance_threshold = settings.context.fact_similarity;

    let records = session
        .search_facts(embedding, limit, distance_threshold)
        .await?;

    if records.is_empty() {
        info!("Search for fact '{query_text}' returned no results.");
        return Ok(format!(
            "No relevant facts found for query: \"{raw_query}\""
        ));
    }

    info!("Found {} facts for query '{raw_query}'", records.len());

    let mut response = format!(
        "Found {} relevant facts for query \"{raw_query}\":\n",
        records.len()
    );

    for record in records {
        response.push_str(&format!("  * [ID: {}] {}\n", record.id, record.data.text));
    }

    Ok(response)
}
