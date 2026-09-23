//! Users management module.

pub mod session;
pub use session::Session;

pub mod metadata;
pub use metadata::{SessionMetadata, UserMetadata};

use std::collections::HashMap;
use std::sync::Arc;

use crate::{prelude::*, utils};

use anylm::embeddings::Search;
use cistern::{Context, ContextRecord, Storage, gen_id};
use osy_share::{SessionId, UserFact, UserRule};

/// User states map {uid => state}.
static USER_STATES: SharedMap<u64, UserState> = SharedMap::new();

const FACTS_TABLE_NAME: &str = "facts";
const RULES_TABLE_NAME: &str = "rules";

/// User state.
pub struct UserState {
    /// Unique user identifier.
    pub id: u64,
    /// Key-Value database for user metadata & rules.
    pub kv_db: Arc<Storage>,
    /// Vector database for user facts.
    pub rag_db: Arc<Context>,
    /// Active user sessions.
    pub sessions: HashMap<SessionId, session::SharedSession>,
}

impl UserState {
    /// Returns user state or initializes it.
    pub async fn get_or_init(id: u64) -> Result<SharedItem<UserState>> {
        // search at active users
        if let Some(user) = USER_STATES.get(&id).await {
            return Ok(user);
        }

        // initializing user
        let user_base = path!("$share$/users/{id}");
        let kv_dir = user_base.join("userdata");
        let kv_db = Arc::new(Storage::connect(kv_dir).await?);

        let context_dir = user_base.join("context");
        let rag_db = Arc::new(Context::connect(context_dir).await?);

        let state = Self {
            id,
            kv_db,
            rag_db,
            sessions: HashMap::new(),
        };

        // insert to global state
        USER_STATES.insert(id, state).await;

        // return atomic reference to state
        USER_STATES.get(&id).await.ok_or_else(|| {
            Error::Custom("Failed to acquire user state lock after initialization".into()).into()
        })
    }

    /// Returns list of user sessions (ordered by freshness).
    pub async fn list_sessions(id: u64, limit: usize) -> Result<Vec<SessionId>> {
        let user = Self::get_or_init(id).await?;
        let guard = user.read().await;

        let meta = guard.load_metadata().await?.unwrap_or_default();
        let mut sessions = meta.sessions;

        sessions.reverse();
        if limit > 0 && sessions.len() > limit {
            sessions.truncate(limit);
        }

        Ok(sessions)
    }

    /// Loads user's metadata from database.
    pub async fn load_metadata(&self) -> Result<Option<UserMetadata>> {
        let table = self.kv_db.open_table("global").await?;
        table.read("user_metadata").await
    }

    /// Retains user's metadata into database.
    pub async fn save_metadata(&self, meta: &UserMetadata) -> Result<()> {
        let table = self.kv_db.open_table("global").await?;

        table.write("user_metadata", meta.clone()).await?;
        table.flush().await?;

        Ok(())
    }

    /// Returns last used session ID.
    pub async fn get_last_session_id(&self) -> Result<Option<SessionId>> {
        let meta = self.load_metadata().await?.unwrap_or_default();
        Ok(meta.last_session)
    }

    /// Removes user sessions.
    pub async fn remove_session(&mut self, session_id: &SessionId) -> Result<()> {
        // remove from memory
        self.sessions.remove(session_id);

        // remove from metadata
        let mut user_meta = self.load_metadata().await?.unwrap_or_default();
        user_meta.sessions.retain(|s| s != session_id);
        if user_meta.last_session == Some(*session_id) {
            user_meta.last_session = user_meta.sessions.last().copied();
        }
        self.save_metadata(&user_meta).await?;

        // clear session database
        let session_table_name = str!(session_id);
        let table = self.kv_db.open_table(&session_table_name).await?;

        table.clear().await?;
        table.flush().await?;

        Ok(())
    }
}

impl UserState {
    /// Returns list of all user's rules (global).
    pub async fn load_rules(&self) -> Result<Vec<UserRule>> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;

        Ok(table
            .read_all::<u64, UserRule>()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.1)
            .collect())
    }

    /// Retains new user rule (global).
    pub async fn save_rule(&self, id: Option<u64>, text: String) -> Result<UserRule> {
        let rule_id = id.unwrap_or_else(gen_id);
        let rule = UserRule {
            id: rule_id,
            text,
            is_global: true,
            created_at: Utc::now(),
        };

        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;

        table.write(rule_id, rule.clone()).await?;
        table.flush().await?;

        Ok(rule)
    }

    /// Removes user rule by ID (global).
    pub async fn remove_rule(&self, id: u64) -> Result<bool> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;

        if table.read::<_, UserRule>(id).await?.is_some() {
            table.remove(id).await?;
            table.flush().await?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Removes all user's rules.
    pub async fn clear_rules(&self) -> Result<()> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;

        table.clear().await?;
        table.flush().await?;

        Ok(())
    }
}

impl UserState {
    /// Retains new user fact (with deduplication).
    pub async fn save_fact(&self, text: impl Into<String>) -> Result<()> {
        let text = text.into();
        if text.trim().is_empty() {
            return Err(Error::Custom("Fact text cannot be empty.".into()).into());
        }

        // generate text embedding
        let search_text = utils::normalize_fact_text(&text).await;
        let embedding = utils::generate_embedding(&search_text, Search::Document).await?;

        // remove duplicate (if exists)
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let dedup_threshold = Config::get().context.dedup_similarity;

        if let Ok(Some(similar_facts)) = table
            .read::<UserFact>(embedding.clone(), Some(5), dedup_threshold)
            .await
        {
            for record in similar_facts {
                if record.data.text.trim().eq_ignore_ascii_case(text.trim()) {
                    return Ok(());
                }
                let _ = table.remove(record.id).await;
            }
        }

        // create fact
        let fact_id = gen_id();
        let fact = UserFact {
            id: fact_id,
            text: text.trim().to_owned(),
            search_text,
            created_at: Utc::now(),
        };

        // write fact to database
        table.write(fact_id, embedding, fact).await?;
        Ok(())
    }

    /// Searches user's facts in database.
    pub async fn search_facts(
        &self,
        query: &str,
        limit: Option<usize>,
    ) -> Result<Vec<ContextRecord<UserFact>>> {
        if query.trim().is_empty() {
            return Err(Error::Custom("Search query cannot be empty.".into()).into());
        }

        // if it’s not English, translate it
        let query = if !utils::is_english(&query) {
            utils::translate_to_english(&query, true).await?
        } else {
            query.trim().to_owned()
        };

        // generate query embedding
        let embedding = utils::generate_embedding(&query, Search::Query).await?;

        let ctx = &Config::get().context;
        let dist = ctx.fact_similarity;

        // search facts in database
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let records = table.read(embedding, limit, dist).await?;

        Ok(records.unwrap_or_default())
    }

    /// Returns list of all user's facts.
    pub async fn load_facts(&self) -> Result<Vec<UserFact>> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let records = table.read_all().await?;

        Ok(records
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.data)
            .collect())
    }

    /// Removes user fact by ID.
    pub async fn remove_fact(&self, fact_id: u64) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.remove(fact_id).await?;
        Ok(())
    }

    /// Removes all user's facts.
    pub async fn clear_facts(&self) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.clear().await?;
        Ok(())
    }
}
