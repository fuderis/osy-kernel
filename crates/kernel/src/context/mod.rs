use crate::prelude::*;
use anylm::{
    api::{Content, Message, Messages, Schema},
    completions::{Chunk, Completions},
    embeddings::{EmbeddingSearch, Embeddings},
};

#[derive(Deserialize)]
struct NormalizedFact {
    search_text: String,
}

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

pub async fn generate_embedding(text: &str, search: EmbeddingSearch) -> Result<Vec<f32>> {
    let ai_ops = Settings::get().embeddings.options.clone();

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

pub fn extract_text_from_msg(msg: &Message) -> Option<String> {
    let text: String = msg
        .content
        .iter()
        .filter_map(|c| match c {
            Content::Text { text } => Some(text.as_str()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join("\n");

    if text.trim().is_empty() {
        None
    } else {
        Some(text)
    }
}
