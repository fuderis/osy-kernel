use crate::{context, prelude::*, session::Session, skills};
use anylm::{
    api::{Messages, Schema, Tool},
    completions::{Chunk, Completions},
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
            "forget_fact",
            "Removes a specific fact from long-term memory by its ID. \
            Use this when a previously remembered fact is obsolete, incorrect, or the user explicitly asks to forget it.",
        )
        .required_property(
            "fact_id",
            Schema::integer("The unique numerical ID of the fact to forget."),
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
pub async fn handle_remember_fact(
    session: &Session,
    action: skills::fact::RememberFactAction,
) -> Result<String> {
    let fact_text = action.text.trim().to_string();
    if fact_text.is_empty() {
        return Ok("Fact text cannot be empty.".into());
    }

    // 1. Нормализуем текст с помощью вынесенной функции
    let search_text = context::normalize_fact_text(&fact_text).await;

    // 2. Генерируем эмбеддинг по нормализованному тексту
    let embedding = context::generate_embedding(&search_text, EmbeddingSearch::Document).await?;

    // 3. Сохраняем факт в базу данных
    session
        .save_fact(embedding, fact_text.clone(), Some(search_text.into()))
        .await?;

    info!("Saved new user fact: '{fact_text}'");
    Ok(format!("Fact successfully saved: \"{fact_text}\""))
}

/// Removes a specific fact from the user's vector storage by its ID
pub async fn handle_forget_fact(session: &Session, fact_id: u64) -> Result<String> {
    session.remove_fact(fact_id).await?;

    info!("Removed user fact #{fact_id}");
    Ok(format!("Fact #{fact_id} successfully removed."))
}

/// Searches for relevant user facts using semantic vector search
pub async fn handle_search_fact(
    session: &Session,
    action: skills::fact::SearchFactAction,
) -> Result<String> {
    #[derive(Deserialize)]
    struct TranslatedQuery {
        translated_text: String,
    }

    let raw_query = action.search_text.trim();
    if raw_query.is_empty() {
        return Ok("Search query is empty.".into());
    }

    // 1. Быстро определяем язык запроса
    let is_english = whatlang::detect(raw_query)
        .map(|info| info.lang() == whatlang::Lang::Eng)
        .unwrap_or(false);

    // 2. Если не английский — переводим через LLM с принудительной JSON Schema
    let query_text = if !is_english {
        let settings = Settings::get();
        let messages = Messages::new()
            .system(vec![
                "You are a translator. Translate the given user search query to English for semantic vector search.".into(),
            ])
            .user(vec![raw_query.into()])
            .wrap();

        let mut response = Completions::try_from(settings.completions.options.clone())?
            .schema(
                Schema::object("Search query translation structure").required_property(
                    "translated_text",
                    Schema::string("Clear English translation of the search query"),
                ),
            )
            .send(messages)
            .await?;

        let mut json_str = String::new();
        while let Some(chunk) = response.next().await {
            if let Chunk::Text(text) = chunk? {
                json_str.push_str(&text);
            }
        }

        // Парсим гарантированный JSON, при ошибке откатываемся на сырой запрос
        serde_json::from_str::<TranslatedQuery>(&json_str)
            .map(|parsed| parsed.translated_text)
            .unwrap_or_else(|e| {
                warn!("Failed to parse translated query JSON, fallback to raw query: {e}");
                raw_query.to_string()
            })
    } else {
        raw_query.to_string()
    };

    // 3. Генерируем эмбеддинг по строго английскому тексту
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
        // Убрано отображение метки [PINNED], так как факты больше не поддерживают закрепление
        response.push_str(&format!("  * [ID: {}] {}\n", record.id, record.data.text));
    }

    Ok(response)
}
