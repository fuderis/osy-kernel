pub mod fact;
pub use fact::*;

use crate::prelude::*;
use anylm::{
    api::{Content, Message},
    embeddings::{EmbeddingSearch, Embeddings},
};

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
