use crate::prelude::*;
use anylm::embeddings::{EmbeddingSearch, Embeddings};

/// Generates the text embeddings
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
