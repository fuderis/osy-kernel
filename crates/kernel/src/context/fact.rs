use crate::{prelude::*, session::Session};
use anylm::embeddings::EmbeddingSearch;

/// The user context fact record
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct UserFact {
    /// Text content of the extracted fact
    pub text: String,
    /// Unix timestamp (in seconds) when the fact was stored
    pub created_at: u64,
}

/// Saves a new fact to the user's vector storage with automatic embedding generation
pub async fn handle_remember(session: &Session, fact_text: String) -> Result<String> {
    let embedding = super::generate_embedding(&fact_text, EmbeddingSearch::Document).await?;
    session.save_fact(embedding, fact_text.clone()).await?;

    info!("Saved new user fact: '{fact_text}'");
    Ok(format!("Fact successfully saved: \"{fact_text}\""))
}

/// Removes a specific fact from the user's vector storage by its ID
pub async fn handle_forget(session: &Session, fact_id: u64) -> Result<String> {
    session.remove_fact(fact_id).await?;

    info!("Removed user fact #{fact_id}");
    Ok(format!("Fact #{fact_id} successfully removed."))
}
