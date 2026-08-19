pub mod key;
use key::Key;

pub mod metadata;
use metadata::Metadata;

use atoman::State;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::Mutex;

use crate::prelude::*;

use anylm::api::Message;
use cistern::{Cistern, Kv, Rag, RagRecord, generate_id};
use osy_share::{SessionId, SessionInfo, UserFact, UserRule};
use serde::{Deserialize, Serialize};

static USER_DBS: State<HashMap<u128, UserState>> = State::default();
static SESSIONS: State<HashMap<SessionId, SharedSession>> = State::default();

const FACTS_TABLE_NAME: &str = "facts";
const RULES_TABLE_NAME: &str = "rules";

/// Combined global databases for a single user
#[derive(Clone)]
pub struct UserState {
    pub kv_db: Arc<Cistern<Kv>>,
    pub rag_db: Arc<Cistern<Rag>>,
}

impl UserState {
    /// Получает закешированный `UserState` из `USER_DBS` или монтирует базы с диска
    pub async fn get_or_init(user_id: u128) -> Result<Self> {
        let user_base = path!("$share$/users/{user_id}");

        let mut user_dbs = USER_DBS.lock().await;
        if let Some(state) = user_dbs.get(&user_id) {
            return Ok(state.clone());
        }

        let kv_dir = user_base.join("userdata");
        let kv_db = arc!(Cistern::connect(kv_dir).await?);

        let context_dir = user_base.join("context");
        let rag_db = arc!(Cistern::connect(context_dir).await?);

        let state = Self { kv_db, rag_db };
        user_dbs.insert(user_id, state.clone());

        Ok(state)
    }

    /// Retrieves all session IDs for a given user ordered by recency
    pub async fn sessions_list(user_id: u128, limit: usize) -> Result<Vec<SessionId>> {
        let user_base = path!("$share$/users/{user_id}");
        let user_kv_dir = user_base.join("userdata");

        // return immediately if the user data directory does not exist
        if !user_kv_dir.exists() {
            return Ok(Vec::new());
        }

        // 1. attempt to reuse an existing user_kv_db handle from USER_DBS, or initialize a new one
        let user_kv_db = {
            let mut user_dbs = USER_DBS.lock().await;

            if let Some(state) = user_dbs.get(&user_id) {
                state.kv_db.clone()
            } else {
                let user_kv_db = arc!(Cistern::connect(user_kv_dir).await?);
                let context_dir = user_base.join("context");
                let rag_db = arc!(Cistern::connect(context_dir).await?);

                let state = UserState {
                    kv_db: user_kv_db.clone(),
                    rag_db,
                };
                user_dbs.insert(user_id, state);

                user_kv_db
            }
        };

        // 2. read user_metadata from the "global" table
        let table = user_kv_db.open_table("global").await?;
        let meta: Option<UserMetadata> = table.read("user_metadata").await?;

        let Some(user_meta) = meta else {
            return Ok(Vec::new());
        };

        let mut sessions = user_meta.sessions;

        // reverse list to order most recent sessions first
        sessions.reverse();

        // apply limit if specified
        if limit > 0 && sessions.len() > limit {
            sessions.truncate(limit);
        }

        Ok(sessions)
    }

    // --- Metadata API ---

    pub async fn get_metadata(&self) -> Result<Option<UserMetadata>> {
        let table = self.kv_db.open_table("global").await?;
        table.read("user_metadata").await
    }

    pub async fn save_metadata(&self, meta: &UserMetadata) -> Result<()> {
        let table = self.kv_db.open_table("global").await?;
        table.write("user_metadata", meta.clone()).await?;
        table.flush().await?;
        Ok(())
    }

    // --- Global Rules API ---

    pub async fn list_global_rules(&self) -> Result<Vec<UserRule>> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        Ok(table
            .read_all::<u64, UserRule>()
            .await
            .unwrap_or_default()
            .into_iter()
            .map(|r| r.1)
            .collect())
    }

    pub async fn save_global_rule(&self, id: Option<u64>, text: String) -> Result<UserRule> {
        let rule_id = id.unwrap_or_else(generate_id);
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

    pub async fn remove_global_rule(&self, id: u64) -> Result<bool> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if table.read::<_, UserRule>(id).await?.is_some() {
            table.remove(id).await?;
            table.flush().await?;
            return Ok(true);
        }
        Ok(false)
    }

    pub async fn clear_global_rules(&self) -> Result<()> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        table.clear().await?;
        table.flush().await?;
        Ok(())
    }

    // --- Facts / RAG API ---

    pub async fn save_fact(
        &self,
        embedding: Vec<f32>,
        text: String,
        search_text: Option<String>,
    ) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let dedup_threshold = Settings::get().context.dedup_similarity;

        if let Ok(Some(similar_facts)) = table
            .read::<UserFact>(embedding.clone(), 5, dedup_threshold)
            .await
        {
            for record in similar_facts {
                if record.data.text.trim().eq_ignore_ascii_case(text.trim()) {
                    return Ok(());
                }
                let _ = table.remove(record.id).await;
            }
        }

        let effective_search_text = search_text
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| text.clone());

        let fact_id = generate_id();
        let fact = UserFact {
            id: fact_id,
            text,
            search_text: effective_search_text,
            created_at: Utc::now(),
        };

        table.write(fact_id, embedding, fact).await?;

        Ok(())
    }

    pub async fn search_facts(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        distance_threshold: f32,
    ) -> Result<Vec<RagRecord<UserFact>>> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let records = table
            .read(query_embedding, limit, distance_threshold)
            .await?;
        Ok(records.unwrap_or_default())
    }

    pub async fn list_all_facts(&self) -> Result<Vec<UserFact>> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let records = table.read_all().await?;
        Ok(records
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.data)
            .collect())
    }

    pub async fn remove_fact(&self, fact_id: u64) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.remove(fact_id).await?;

        Ok(())
    }

    pub async fn clear_all_facts(&self) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.clear().await?;

        Ok(())
    }
}

type SharedSession = Arc<Mutex<Session>>;

/// Global metadata persisted per user in `$share$/users/{uid}/userdata`
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct UserMetadata {
    /// List of all created session IDs for this user
    pub sessions: Vec<SessionId>,
    /// The ID of the most recently active or created session
    pub last_session: Option<SessionId>,
}

/// The user session manager
#[derive(Clone)]
pub struct Session {
    /// Unique identifier of the active session
    pub id: SessionId,
    /// Runtime environment info and metadata for the session
    pub info: SessionInfo,
    /// Key-Value storage instance for session chat history and session-level metadata
    pub kv_db: Arc<Cistern<Kv>>,
    /// Global Key-Value storage instance for user-wide settings and session registry
    pub user_kv_db: Arc<Cistern<Kv>>,
    /// Vector RAG database instance for user-specific long-term memory/facts
    pub rag_db: Arc<Cistern<Rag>>,
}

impl Session {
    /// Initializes the user session instance and updates global user state
    pub async fn init(id: SessionId, info: SessionInfo) -> Result<SharedSession> {
        let user_base = path!("$share$/users/{}", id.user_id);
        let uid = id.user_id as u128;

        // 1. Извлекаем глобальный UserDbState
        let user_state = UserState::get_or_init(uid).await?;

        // 2. Монтируем БД конкретной сессии
        let session_dir = user_base.join("sessions").join(id.to_string());
        let kv_db = arc!(Cistern::connect(session_dir).await?);

        let this = arc_mutex!(Self {
            id,
            info,
            kv_db,
            user_kv_db: user_state.kv_db.clone(),
            rag_db: user_state.rag_db.clone(),
        });

        // 3. Обновляем сессионный реестр в metadata
        let user_meta_res = user_state.get_metadata().await?;
        let mut user_meta = user_meta_res.unwrap_or_default();

        if !user_meta.sessions.contains(&id) {
            user_meta.sessions.push(id);
        }
        user_meta.last_session = Some(id);
        user_state.save_metadata(&user_meta).await?;

        SESSIONS.lock().await.insert(id, this.clone());
        Ok(this)
    }

    /// Возвращает абстракцию `UserState` для данной сессии
    pub fn user_state(&self) -> UserState {
        UserState {
            kv_db: self.user_kv_db.clone(),
            rag_db: self.rag_db.clone(),
        }
    }

    /// Returns the active user session instance from memory
    pub fn get(id: &SessionId) -> Option<SharedSession> {
        SESSIONS.dirty_get().get(id).map(Clone::clone)
    }

    /// Finishes the user session and flushes pending writes
    pub async fn finish(id: &SessionId) -> Result<()> {
        if let Some(session) = SESSIONS.lock().await.remove(id) {
            let user_id = id.user_id as u128;
            let session_guard = session.lock().await;

            // flush pending changes in session-specific KV table
            let table_name = str!(id);
            let table = session_guard.kv_db.open_table(&table_name).await?;
            table.flush().await?;

            // flush pending changes in global user_kv table
            let user_table = session_guard.user_kv_db.open_table("global").await?;
            user_table.flush().await?;

            let user_kv_db = session_guard.user_kv_db.clone();

            drop(session_guard);
            drop(session);

            // check whether the closing session is the last active instance for this user.
            // strong_count == 2 means: 1 reference inside USER_DBS + 1 local variable reference
            let mut user_dbs = USER_DBS.lock().await;
            if Arc::strong_count(&user_kv_db) <= 2 {
                user_dbs.remove(&user_id);
            }
        }
        Ok(())
    }

    // --- Global User State API ---

    /// Reads global user metadata
    pub async fn get_user_metadata(&self) -> Result<Option<UserMetadata>> {
        let table = self.user_kv_db.open_table("global").await?;
        table.read("user_metadata").await
    }

    /// Saves global user metadata
    pub async fn save_user_metadata(&self, meta: &UserMetadata) -> Result<()> {
        let table = self.user_kv_db.open_table("global").await?;
        table.write("user_metadata", meta.clone()).await?;
        table.flush().await?;
        Ok(())
    }

    /// Returns the list of all session IDs owned by this user
    pub async fn list_sessions(&self) -> Result<Vec<SessionId>> {
        let meta = self.get_user_metadata().await?.unwrap_or_default();
        Ok(meta.sessions)
    }

    /// Returns the ID of the last active session
    pub async fn get_last_session_id(&self) -> Result<Option<SessionId>> {
        let meta = self.get_user_metadata().await?.unwrap_or_default();
        Ok(meta.last_session)
    }

    /// Deletes a session completely from the registry and removes its data
    pub async fn remove_session(&self, session_id: &SessionId) -> Result<()> {
        // remove from active runtime sessions if loaded
        SESSIONS.lock().await.remove(session_id);

        // remove from global user metadata
        let mut user_meta = self.get_user_metadata().await?.unwrap_or_default();
        user_meta.sessions.retain(|s| s != session_id);
        if user_meta.last_session == Some(*session_id) {
            user_meta.last_session = user_meta.sessions.last().copied();
        }
        self.save_user_metadata(&user_meta).await?;

        // clear individual session DB
        let session_table_name = str!(session_id);
        let table = self.kv_db.open_table(&session_table_name).await?;
        table.clear().await?;
        table.flush().await?;

        Ok(())
    }

    // --- Session Message API ---

    /// Reads the session-level metadata
    pub async fn read_metadata(&self) -> Result<Option<Metadata>> {
        let table_name = str!(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        table.read(Key::Metadata).await
    }

    /// Reads all messages within the current session
    pub async fn read_messages(&self) -> Result<Vec<Message>> {
        let table_name = str!(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let meta = match table.read(Key::Metadata).await? {
            Some(meta) => meta,
            None => {
                let new_meta = Metadata::new(self.id);
                table.write(Key::Metadata, new_meta.clone()).await?;
                table.flush().await?;
                new_meta
            }
        };

        let start_idx = meta.compressed_until;
        let end_idx = meta.message_count as usize;

        let mut messages = Vec::with_capacity(end_idx.saturating_sub(start_idx));
        for i in start_idx..end_idx {
            let msg_key = Key::Message(i);
            if let Some(msg) = table.read::<_, Message>(msg_key).await? {
                messages.push(msg);
            }
        }

        Ok(messages)
    }

    /// Writes a new message to the current session
    pub async fn write_message(&self, message: Message) -> Result<()> {
        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let mut meta: Metadata = table
            .read(Key::Metadata)
            .await?
            .unwrap_or(Metadata::new(self.id));

        let msg_key = Key::Message(meta.message_count as usize);
        table.write(msg_key, message).await?;

        meta.message_count += 1;
        table.write(Key::Metadata, meta).await?;
        table.flush().await?;

        Ok(())
    }

    /// Writes multiple messages to the current session
    pub async fn write_messages(&self, messages: Vec<Message>) -> Result<()> {
        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let mut meta: Metadata = table
            .read(Key::Metadata)
            .await?
            .unwrap_or(Metadata::new(self.id));

        for message in messages {
            let msg_key = Key::Message(meta.message_count as usize);
            table.write(msg_key, message).await?;
            meta.message_count += 1;
        }

        table.write(Key::Metadata, meta).await?;
        table.flush().await?;

        Ok(())
    }

    /// Inserts a compressed message and shifts the remaining active history
    pub async fn insert_and_shift(
        &self,
        compressed_msg: Message,
        preserve_msgs: Vec<Message>,
        compress_count: usize,
    ) -> Result<()> {
        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let current_meta = table
            .read::<_, Metadata>(Key::Metadata)
            .await?
            .unwrap_or(Metadata::new(self.id));

        let insert_idx = current_meta.compressed_until + compress_count;
        let mut current_idx = insert_idx;

        table
            .write(Key::Message(current_idx), compressed_msg)
            .await?;
        current_idx += 1;

        for msg in preserve_msgs {
            table.write(Key::Message(current_idx), msg).await?;
            current_idx += 1;
        }

        let new_message_count = std::cmp::max(current_meta.message_count, current_idx as u64);
        let new_meta = Metadata {
            session_id: self.id,
            message_count: new_message_count,
            compressed_until: insert_idx,
        };

        table.write(Key::Metadata, new_meta).await?;
        table.flush().await?;

        Ok(())
    }

    /// Completely clears current session message history
    pub async fn clear(&self) -> Result<()> {
        let table_name = str!(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        if let Some(meta) = table.read::<_, Metadata>(Key::Metadata).await? {
            let start_idx = meta.compressed_until;
            let end_idx = meta.message_count as usize;

            for i in start_idx..end_idx {
                table.remove(Key::Message(i)).await?;
            }
        }

        let fresh_meta = Metadata::new(self.id);
        table.write(Key::Metadata, fresh_meta).await?;
        table.flush().await?;

        Ok(())
    }
}

// --- Rules Management API (Session & Global) ---

impl Session {
    /// Saves or updates a rule (overwrites if ID already exists).
    /// Generates a new u64 ID automatically if `id` is None.
    pub async fn save_rule(
        &self,
        id: Option<u64>,
        text: String,
        is_global: bool,
    ) -> Result<UserRule> {
        let rule_id = id.unwrap_or_else(generate_id);
        let rule = UserRule {
            id: rule_id,
            text,
            is_global,
            created_at: Utc::now(),
        };

        if is_global {
            let table = self.user_kv_db.open_table(RULES_TABLE_NAME).await?;
            table.write(rule_id, rule.clone()).await?;
            table.flush().await?;
        } else {
            let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
            table.write(rule_id, rule.clone()).await?;
            table.flush().await?;
        }

        Ok(rule)
    }

    /// Fetches a rule by its numeric ID (checks session KV first, then falls back to global)
    pub async fn get_rule(&self, id: u64) -> Result<Option<UserRule>> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Some(rule) = local_table.read::<_, UserRule>(id).await? {
            return Ok(Some(rule));
        }

        let global_table = self.user_kv_db.open_table(RULES_TABLE_NAME).await?;
        global_table.read::<_, UserRule>(id).await
    }

    /// Deletes a rule by ID (checks both local and global tables)
    pub async fn remove_rule(&self, id: u64) -> Result<bool> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if local_table.read::<_, UserRule>(id).await?.is_some() {
            local_table.remove(id).await?;
            local_table.flush().await?;
            return Ok(true);
        }

        let global_table = self.user_kv_db.open_table(RULES_TABLE_NAME).await?;
        if global_table.read::<_, UserRule>(id).await?.is_some() {
            global_table.remove(id).await?;
            global_table.flush().await?;
            return Ok(true);
        }

        Ok(false)
    }

    /// Returns a flat list of all active rules for the current session.
    pub async fn list_session_rules(&self) -> Result<Vec<UserRule>> {
        let mut rules = Vec::new();

        let global_table = self.user_kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Ok(global_rules) = global_table.read_all::<u64, UserRule>().await {
            rules.extend(global_rules);
        }

        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Ok(local_rules) = local_table.read_all::<u64, UserRule>().await {
            rules.extend(local_rules);
        }

        Ok(rules.into_iter().map(|r| r.1).collect())
    }

    /// Returns both local and global user rules aggregated together
    pub async fn list_all_rules(&self) -> Result<Vec<UserRule>> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        let local_rules = local_table
            .read_all::<u64, UserRule>()
            .await
            .unwrap_or_default();

        let global_table = self.user_kv_db.open_table(RULES_TABLE_NAME).await?;
        let global_rules = global_table
            .read_all::<u64, UserRule>()
            .await
            .unwrap_or_default();

        Ok(vec![local_rules, global_rules]
            .into_iter()
            .flatten()
            .map(|r| r.1)
            .collect::<Vec<_>>())
    }

    /// Clears all local rules for the current session
    pub async fn clear_local_rules(&self) -> Result<()> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        table.clear().await?;
        table.flush().await?;
        Ok(())
    }

    /// Clears all global user rules
    pub async fn clear_global_rules(&self) -> Result<()> {
        let table = self.user_kv_db.open_table(RULES_TABLE_NAME).await?;
        table.clear().await?;
        table.flush().await?;
        Ok(())
    }
}

// --- Global User Context / RAG API (Facts) ---

impl Session {
    /// Saves a new user fact, removes close duplicates, and updates user metadata metrics
    pub async fn save_fact(
        &self,
        embedding: Vec<f32>,
        text: String,
        search_text: Option<String>,
    ) -> Result<()> {
        let table_name = FACTS_TABLE_NAME;
        let table = self.rag_db.open_table(&table_name).await?;
        let dedup_threshold = Settings::get().context.dedup_similarity;

        // search for existing close duplicates
        if let Ok(Some(similar_facts)) = table
            .read::<UserFact>(embedding.clone(), 5, dedup_threshold)
            .await
        {
            for record in similar_facts {
                if record.data.text.trim().eq_ignore_ascii_case(text.trim()) {
                    return Ok(());
                }
                let _ = table.remove(record.id).await;
            }
        }

        let effective_search_text = search_text
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| text.clone());

        let now = Utc::now();
        let fact_id = generate_id();
        let fact = UserFact {
            id: fact_id,
            text,
            search_text: effective_search_text,
            created_at: now,
        };

        table.write(fact_id, embedding, fact).await?;

        Ok(())
    }

    /// Searches for relevant user facts across the user context
    pub async fn search_facts(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        distance_threshold: f32,
    ) -> Result<Vec<RagRecord<UserFact>>> {
        let table_name = FACTS_TABLE_NAME;
        let table = self.rag_db.open_table(&table_name).await?;

        let records = table
            .read(query_embedding, limit, distance_threshold)
            .await?;

        Ok(records.unwrap_or_default())
    }

    /// Fetches all user facts stored in RAG context
    pub async fn list_all_facts(&self) -> Result<Vec<UserFact>> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let records = table.read_all().await?;

        Ok(records
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.data)
            .collect())
    }

    /// Removes a fact by its ID and updates user metadata metrics
    pub async fn remove_fact(&self, fact_id: u64) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;

        table.remove(fact_id).await?;

        Ok(())
    }

    /// Clears all user facts from context
    pub async fn clear_all_facts(&self) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.clear().await?;

        Ok(())
    }
}

impl Session {
    /// Clones the current session: copies all messages, session metadata, and local rules
    /// under a new `SessionId`, registers it in the user’s metadata, and returns the new `SessionId`.
    pub async fn duplicate(&self) -> Result<SessionId> {
        let new_id = SessionId::new(self.id.user_id);

        // 1. Создаем директорию и инициализируем KV БД для новой сессии
        let user_base = path!("$share$/users/{}", new_id.user_id);
        let new_session_dir = user_base.join("sessions").join(new_id.to_string());
        let new_kv_db = arc!(Cistern::<Kv>::connect(new_session_dir).await?);

        let src_table_name = str!(self.id);
        let dst_table_name = str!(new_id);

        let src_table = self.kv_db.open_table(&src_table_name).await?;
        let dst_table = new_kv_db.open_table(&dst_table_name).await?;

        // 2. Копируем Metadata
        let meta = src_table.read::<_, Metadata>(Key::Metadata).await?;
        if let Some(mut meta) = meta {
            let start_idx = meta.compressed_until;
            let end_idx = meta.message_count as usize;

            // Копируем только актуальные сообщения с СОХРАНЕНИЕМ исходных индексов
            for i in start_idx..end_idx {
                let msg_key = Key::Message(i);
                if let Some(msg) = src_table.read::<_, Message>(msg_key.clone()).await? {
                    dst_table.write(msg_key, msg).await?;
                }
            }

            meta.session_id = new_id;
            dst_table.write(Key::Metadata, meta).await?;
        }

        dst_table.flush().await?;

        // 3. Копируем локальные правила сессии
        let src_rules_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        let dst_rules_table = new_kv_db.open_table(RULES_TABLE_NAME).await?;

        if let Ok(local_rules) = src_rules_table.read_all::<u64, UserRule>().await {
            for (rule_id, rule) in local_rules {
                dst_rules_table.write(rule_id, rule).await?;
            }
            dst_rules_table.flush().await?;
        }

        // 4. Собираем экземпляр новой сессии
        let new_session = arc_mutex!(Self {
            id: new_id,
            info: self.info.clone(),
            kv_db: new_kv_db,
            user_kv_db: self.user_kv_db.clone(),
            rag_db: self.rag_db.clone(),
        });

        // 5. Регистрируем новую сессию в глобальных метаданных пользователя
        let user_state = self.user_state();
        let mut user_meta = user_state.get_metadata().await?.unwrap_or_default();

        if !user_meta.sessions.contains(&new_id) {
            user_meta.sessions.push(new_id);
        }
        user_meta.last_session = Some(new_id);
        user_state.save_metadata(&user_meta).await?;

        // 6. Сохраняем новую сессию в памяти (SESSIONS map)
        SESSIONS.lock().await.insert(new_id, new_session);

        Ok(new_id)
    }
}
