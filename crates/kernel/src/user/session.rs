use super::*;
use crate::prelude::*;

use anylm::api::Message;
use cistern::{Cistern, Kv, RagRecord, generate_id};
use osy_share::{SessionId, SessionInfo, UserFact, UserRule};
use std::sync::Arc;
use tokio::sync::Mutex;

pub type SharedSession = Arc<Mutex<Session>>;

/// The user session manager
pub struct Session {
    /// Unique identifier of the active session
    pub id: SessionId,
    /// Runtime environment info and metadata for the session
    pub info: SessionInfo,
    /// Key-Value storage instance for session chat history and session-level metadata
    pub kv_db: Arc<Cistern<Kv>>,
    /// ID владельца сессии для быстрого доступа к UserState
    pub user_id: u64,
}

impl Session {
    /// Initializes the user session instance and registers it within `UserState`
    pub async fn init(id: SessionId, info: SessionInfo) -> Result<SharedSession> {
        let uid = id.user_id as u64;

        let user_base = path!("$share$/users/{}", id.user_id);
        let session_dir = user_base.join("sessions").join(id.to_string());
        let kv_db = Arc::new(Cistern::connect(session_dir).await?);

        let this = Arc::new(Mutex::new(Self {
            id,
            info,
            kv_db,
            user_id: uid,
        }));

        // Изолируем скоуп блокировки UserState
        {
            let mut user_guard = UserState::get_or_init(uid).await?;

            // ПРЯМОЙ вызов методов у user_guard без вызова self.save_user_metadata()
            let mut user_meta = user_guard.get_metadata().await?.unwrap_or_default();
            if !user_meta.sessions.contains(&id) {
                user_meta.sessions.push(id);
            }
            user_meta.last_session = Some(id);

            user_guard.save_metadata(&user_meta).await?;
            user_guard.sessions.insert(id, this.clone());
        } // <- ДРОПАЕМ user_guard ЗДЕСЬ! Блокировка W-Lock снята.

        Ok(this)
    }

    /// Returns active user session instance from memory
    pub async fn get(id: &SessionId) -> Option<SharedSession> {
        let uid = id.user_id as u64;

        // Чистое R-lock обращение через UserState API
        let user_guard = UserState::get_or_init_read(uid).await.ok()?;
        user_guard.sessions.get(id).cloned()
    }

    /// Finishes user session, flushes pending writes and cleans up memory if last session
    pub async fn finish(id: &SessionId) -> Result<()> {
        let uid = id.user_id as u64;

        // 1. Извлекаем сессию из карты под W-lock и мгновенно отпускаем гард
        let (removed_session, is_empty) = {
            if let Ok(mut user_guard) = UserState::get_or_init(uid).await {
                let session = user_guard.sessions.remove(id);
                let empty = user_guard.sessions.is_empty();
                (session, empty)
            } else {
                (None, false)
            }
        };

        // 2. I/O операции над сессией выполняются БЕЗ удержания блокировки карты пользователей
        if let Some(session) = removed_session {
            let session_guard = session.lock().await;

            let table_name = str!(id);
            let table = session_guard.kv_db.open_table(&table_name).await?;
            table.flush().await?;
        }

        // 3. Если активных сессий не осталось, аккуратно удаляем UserState из памяти
        if is_empty {
            if let Ok(user_guard) = UserState::get_or_init(uid).await {
                if user_guard.sessions.is_empty() {
                    drop(user_guard);
                    // Разблокировали элемент, теперь можем удалить его из шардированной карты
                    USER_STATES.remove(&uid).await;
                }
            }
        }

        Ok(())
    }

    // --- Global User State API Delegates ---

    pub async fn get_user_metadata(&self) -> Result<Option<UserMetadata>> {
        let user_guard = UserState::get_or_init_read(self.user_id).await?;
        user_guard.get_metadata().await
    }

    pub async fn save_user_metadata(&self, meta: &UserMetadata) -> Result<()> {
        let user_guard = UserState::get_or_init(self.user_id).await?;
        user_guard.save_metadata(meta).await
    }

    pub async fn list_sessions(&self) -> Result<Vec<SessionId>> {
        let meta = self.get_user_metadata().await?.unwrap_or_default();
        Ok(meta.sessions)
    }

    pub async fn get_last_session_id(&self) -> Result<Option<SessionId>> {
        let meta = self.get_user_metadata().await?.unwrap_or_default();
        Ok(meta.last_session)
    }

    pub async fn remove_session(&self, session_id: &SessionId) -> Result<()> {
        {
            let mut user_guard = UserState::get_or_init(self.user_id).await?;
            user_guard.sessions.remove(session_id);

            let mut user_meta = user_guard.get_metadata().await?.unwrap_or_default();
            user_meta.sessions.retain(|s| s != session_id);
            if user_meta.last_session == Some(*session_id) {
                user_meta.last_session = user_meta.sessions.last().copied();
            }
            user_guard.save_metadata(&user_meta).await?;
        }

        let session_table_name = str!(session_id);
        let table = self.kv_db.open_table(&session_table_name).await?;
        table.clear().await?;
        table.flush().await?;

        Ok(())
    }

    // --- Session Message API ---

    pub async fn read_metadata(&self) -> Result<Option<SessionMetadata>> {
        let table_name = str!(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;
        table.read(Key::Metadata).await
    }

    pub async fn read_messages(&self) -> Result<Vec<Message>> {
        let table_name = str!(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let meta = match table.read(Key::Metadata).await? {
            Some(meta) => meta,
            None => {
                let new_meta = SessionMetadata::new(self.id);
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

    pub async fn write_message(&self, message: Message) -> Result<()> {
        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let mut meta: SessionMetadata = table
            .read(Key::Metadata)
            .await?
            .unwrap_or(SessionMetadata::new(self.id));

        let msg_key = Key::Message(meta.message_count as usize);
        table.write(msg_key, message).await?;

        meta.message_count += 1;
        table.write(Key::Metadata, meta).await?;
        table.flush().await?;

        Ok(())
    }

    pub async fn write_messages(&self, messages: Vec<Message>) -> Result<()> {
        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let mut meta: SessionMetadata = table
            .read(Key::Metadata)
            .await?
            .unwrap_or(SessionMetadata::new(self.id));

        for message in messages {
            let msg_key = Key::Message(meta.message_count as usize);
            table.write(msg_key, message).await?;
            meta.message_count += 1;
        }

        table.write(Key::Metadata, meta).await?;
        table.flush().await?;

        Ok(())
    }

    pub async fn insert_and_shift(
        &self,
        compressed_msg: Message,
        preserve_msgs: Vec<Message>,
        compress_count: usize,
    ) -> Result<()> {
        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let current_meta = table
            .read::<_, SessionMetadata>(Key::Metadata)
            .await?
            .unwrap_or(SessionMetadata::new(self.id));

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
        let new_meta = SessionMetadata {
            session_id: self.id,
            message_count: new_message_count,
            compressed_until: insert_idx,
        };

        table.write(Key::Metadata, new_meta).await?;
        table.flush().await?;

        Ok(())
    }

    pub async fn clear(&self) -> Result<()> {
        let table_name = str!(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        if let Some(meta) = table.read::<_, SessionMetadata>(Key::Metadata).await? {
            let start_idx = meta.compressed_until;
            let end_idx = meta.message_count as usize;

            for i in start_idx..end_idx {
                table.remove(Key::Message(i)).await?;
            }
        }

        let fresh_meta = SessionMetadata::new(self.id);
        table.write(Key::Metadata, fresh_meta).await?;
        table.flush().await?;

        Ok(())
    }

    // --- Rules Management API ---

    pub async fn save_rule(
        &self,
        id: Option<u64>,
        text: String,
        is_global: bool,
    ) -> Result<UserRule> {
        if is_global {
            let user_guard = UserState::get_or_init(self.user_id).await?;
            user_guard.save_global_rule(id, text).await
        } else {
            let rule_id = id.unwrap_or_else(generate_id);
            let rule = UserRule {
                id: rule_id,
                text,
                is_global: false,
                created_at: Utc::now(),
            };
            let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
            table.write(rule_id, rule.clone()).await?;
            table.flush().await?;
            Ok(rule)
        }
    }

    pub async fn get_rule(&self, id: u64) -> Result<Option<UserRule>> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Some(rule) = local_table.read::<_, UserRule>(id).await? {
            return Ok(Some(rule));
        }

        let user_guard = UserState::get_or_init_read(self.user_id).await?;
        let global_table = user_guard.kv_db.open_table(RULES_TABLE_NAME).await?;
        global_table.read::<_, UserRule>(id).await
    }

    pub async fn remove_rule(&self, id: u64) -> Result<bool> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if local_table.read::<_, UserRule>(id).await?.is_some() {
            local_table.remove(id).await?;
            local_table.flush().await?;
            return Ok(true);
        }

        let user_guard = UserState::get_or_init(self.user_id).await?;
        user_guard.remove_global_rule(id).await
    }

    pub async fn list_session_rules(&self) -> Result<Vec<UserRule>> {
        let user_guard = UserState::get_or_init_read(self.user_id).await?;
        let mut rules = user_guard.list_global_rules().await?;

        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Ok(local_rules) = local_table.read_all::<u64, UserRule>().await {
            rules.extend(local_rules.into_iter().map(|r| r.1));
        }

        Ok(rules)
    }

    pub async fn clear_local_rules(&self) -> Result<()> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        table.clear().await?;
        table.flush().await?;
        Ok(())
    }

    pub async fn clear_global_rules(&self) -> Result<()> {
        let user_guard = UserState::get_or_init(self.user_id).await?;
        user_guard.clear_global_rules().await
    }

    // --- RAG / Facts API Delegates ---

    pub async fn save_fact(
        &self,
        embedding: Vec<f32>,
        text: String,
        search_text: Option<String>,
    ) -> Result<()> {
        let user_guard = UserState::get_or_init(self.user_id).await?;
        user_guard.save_fact(embedding, text, search_text).await
    }

    pub async fn search_facts(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        distance_threshold: f32,
    ) -> Result<Vec<RagRecord<UserFact>>> {
        let user_guard = UserState::get_or_init_read(self.user_id).await?;
        user_guard
            .search_facts(query_embedding, limit, distance_threshold)
            .await
    }

    pub async fn list_all_facts(&self) -> Result<Vec<UserFact>> {
        let user_guard = UserState::get_or_init_read(self.user_id).await?;
        user_guard.list_all_facts().await
    }

    pub async fn remove_fact(&self, fact_id: u64) -> Result<()> {
        let user_guard = UserState::get_or_init(self.user_id).await?;
        user_guard.remove_fact(fact_id).await
    }

    pub async fn clear_all_facts(&self) -> Result<()> {
        let user_guard = UserState::get_or_init(self.user_id).await?;
        user_guard.clear_all_facts().await
    }

    // --- Duplication API ---

    pub async fn duplicate(&self) -> Result<SessionId> {
        let new_id = SessionId::new(self.id.user_id);

        let user_base = path!("$share$/users/{}", new_id.user_id);
        let new_session_dir = user_base.join("sessions").join(new_id.to_string());
        let new_kv_db = Arc::new(Cistern::<Kv>::connect(new_session_dir).await?);

        let src_table_name = str!(self.id);
        let dst_table_name = str!(new_id);

        let src_table = self.kv_db.open_table(&src_table_name).await?;
        let dst_table = new_kv_db.open_table(&dst_table_name).await?;

        if let Some(mut meta) = src_table.read::<_, SessionMetadata>(Key::Metadata).await? {
            let start_idx = meta.compressed_until;
            let end_idx = meta.message_count as usize;

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

        let src_rules_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        let dst_rules_table = new_kv_db.open_table(RULES_TABLE_NAME).await?;

        if let Ok(local_rules) = src_rules_table.read_all::<u64, UserRule>().await {
            for (rule_id, rule) in local_rules {
                dst_rules_table.write(rule_id, rule).await?;
            }
            dst_rules_table.flush().await?;
        }

        let new_session = Arc::new(Mutex::new(Self {
            id: new_id,
            info: self.info.clone(),
            kv_db: new_kv_db,
            user_id: self.user_id,
        }));

        {
            let mut user_guard = UserState::get_or_init(self.user_id).await?;
            let mut user_meta = user_guard.get_metadata().await?.unwrap_or_default();

            if !user_meta.sessions.contains(&new_id) {
                user_meta.sessions.push(new_id);
            }
            user_meta.last_session = Some(new_id);
            user_guard.save_metadata(&user_meta).await?;

            user_guard.sessions.insert(new_id, new_session);
        }

        Ok(new_id)
    }
}
