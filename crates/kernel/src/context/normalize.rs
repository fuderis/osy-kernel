use crate::prelude::*;

use anylm::{
    api::{Messages, Schema},
    completions::{Chunk, Completions},
};

/// Normalizes the user fact
pub async fn normalize_fact_text(raw_text: &str) -> String {
    let settings = Settings::get();

    let messages = Messages::new()
        .system(vec![settings.completions.normalize_prompt.clone().into()])
        .user(vec![raw_text.into()])
        .wrap();

    let res = async {
        let mut response = Completions::try_from(settings.completions.options.clone())?
            .schema(
                Schema::object("Normalized fact search structure").required_property(
                    "search_text",
                    Schema::string("Normalized search text for embeddings"),
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

        #[derive(Deserialize)]
        struct NormalizedFact {
            search_text: String,
        }

        let parsed: NormalizedFact = serde_json::from_str(&json_str)?;
        Ok::<String, DynError>(parsed.search_text)
    }
    .await;

    match res {
        Ok(normalized) => normalized,
        Err(e) => {
            warn!("Failed to normalize fact via LLM, fallback to raw text: {e}");
            raw_text.to_string()
        }
    }
}

/// Translates text into English
pub async fn translate_into_english(text: &str) -> Result<String> {
    let settings = Settings::get();
    let messages = Messages::new()
            .system(vec![
                "You are a translator. Translate the given user search query to English for semantic vector search.".into(),
            ])
            .user(vec![text.into()])
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

    #[derive(Deserialize)]
    struct TranslatedQuery {
        translated_text: String,
    }

    Ok(serde_json::from_str::<TranslatedQuery>(&json_str)
        .map(|parsed| parsed.translated_text)
        .map_err(|e| format!("Failed to parse translated query JSON: {e}"))?)
}
