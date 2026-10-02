use crate::prelude::*;

use anylm::{
    IntoSchema, Messages, Schema,
    completions::{Chunk, Completions},
    embeddings::{Embeddings, Search},
};

/// Generates the text embeddings.
pub async fn generate_embedding(text: &str, search: Search) -> Result<Vec<f32>> {
    let ai_ops = Config::get().embeddings.options.clone();

    let embeddings = Embeddings::try_from(ai_ops)?
        .input(text)
        .search(search)
        .send()
        .await?;

    let first = embeddings
        .data
        .into_iter()
        .next()
        .ok_or(Error::NoEmbeddingReceived)?;

    Ok(first.embedding)
}

/// Normalizes user fact text.
pub async fn normalize_fact_text(raw_text: &str) -> String {
    let cfg = &Config::get();
    let normalize_prompt = &cfg.prompts.normalize_prompt;
    let provider_options = &cfg.completions.options;

    let messages = Messages::new()
        .system(vec![normalize_prompt.as_str().into()])
        .user(vec![raw_text.into()])
        .wrap();

    /// Normalized fact search structure.
    #[derive(Deserialize, Schema)]
    struct NormalizedFact {
        /// Normalized search text for embeddings.
        search_text: String,
    }

    let res = async {
        let mut response = Completions::try_from(provider_options.clone())?
            .schema(NormalizedFact::schema())
            .send(messages)
            .await?;

        let mut json_str = String::new();
        while let Some(chunk) = response.next().await {
            if let Chunk::Text(text) = chunk? {
                json_str.push_str(&text);
            }
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

/// Translates text to English.
pub async fn translate_to_english(text: &str, vec_search: bool) -> Result<String> {
    let cfg = Config::get();
    let translate_prompt = &cfg.prompts.translate_prompt;
    let provider_options = &cfg.completions.options;

    let messages = Messages::new()
        .system(vec![
            format!(
                "{}{}",
                translate_prompt.trim(),
                if vec_search {
                    "Optimize for semantic vector search."
                } else {
                    ""
                }
            )
            .into(),
        ])
        .user(vec![text.into()])
        .wrap();

    /// Search query translation structure.
    #[derive(Deserialize, Schema)]
    struct TranslatedQuery {
        /// Clear English translation of the search query.
        translated_text: String,
    }

    let mut response = Completions::try_from(provider_options.clone())?
        .schema(TranslatedQuery::schema())
        .send(messages)
        .await?;

    let mut json_str = String::new();
    while let Some(chunk) = response.next().await {
        if let Chunk::Text(text) = chunk? {
            json_str.push_str(&text);
        }
    }

    Ok(serde_json::from_str::<TranslatedQuery>(&json_str)
        .map(|parsed| parsed.translated_text)
        .map_err(|e| Error::Titled("Failed to parse translated query JSON".into(), e.into()))?)
}
