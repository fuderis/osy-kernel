use super::*;
use crate::prelude::*;

use anylm::api::{Message, Messages};
use cistern::{Storage, gen_id};
use osy_share::{SessionId, SessionInfo, UserRule};

pub type SharedSession = Arc<Mutex<Session>>;

/// Session table key.
#[derive(Clone, Debug, Serialize, Deserialize, Eq, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Key {
    /// Session metadata.
    Metadata,
    /// Session message ID.
    Message(usize),
}

/// User session manager.
pub struct Session {
    /// Unique session identifier.
    pub id: SessionId,
    /// Session info and metadata.
    pub info: SessionInfo,
    /// Key-Value storage for session metadata and history.
    pub kv_db: Arc<Storage>,
    /// User identifier.
    pub user_id: u64,
}

impl Session {
    /// Reads user session from database.
    #[log(sid = %sid)]
    pub async fn read(sid: SessionId) -> Result<(Arc<Mutex<Session>>, Arc<Mutex<Messages>>)> {
        info!("[Session] Reading user session `{sid}`...");

        let Some(session) = Self::get(&sid).await else {
            return Err(Error::UnknownSessionId(sid).into());
        };
        let db_messages = session.lock().await.read_messages().await?;
        let messages = Arc::new(Mutex::new(Messages::from(db_messages)));

        Ok((session, messages))
    }

    /// Initializes user session.
    #[log(sid = %sid)]
    pub async fn init(sid: SessionId, info: SessionInfo) -> Result<SharedSession> {
        info!("[Session] Initializing session `{sid}`...");

        let uid = sid.user_id as u64;

        let user_base = path!("$share$/users/{uid}");
        let session_dir = user_base.join("sessions").join(sid.to_string());
        let kv_db = Arc::new(Storage::connect(session_dir).await?);

        let this = Arc::new(Mutex::new(Self {
            id: sid,
            info,
            kv_db,
            user_id: uid,
        }));

        {
            let user = UserState::get_or_init(uid).await?;
            let mut user_guard = user.write().await;

            let mut user_meta = user_guard.load_metadata().await?.unwrap_or_default();
            if !user_meta.sessions.contains(&sid) {
                user_meta.sessions.push(sid);
            }
            user_meta.last_session = Some(sid);

            user_guard.save_metadata(&user_meta).await?;
            user_guard.sessions.insert(sid, this.clone());
        }

        info!("[Session] Session initialized successfully.");

        Ok(this)
    }

    /// Returns active session from memory.
    pub async fn get(sid: &SessionId) -> Option<SharedSession> {
        let uid = sid.user_id as u64;

        let user = UserState::get_or_init(uid).await.ok()?;
        let user_guard = user.read().await;

        user_guard.sessions.get(sid).cloned()
    }

    /// Finishes user session, flushes and cleans up memory.
    /// If session belongs to temporary user (uid == 0), completely purges it from disk.
    #[log(sid = %sid)]
    pub async fn finish(sid: &SessionId) -> Result<()> {
        info!("[Session] Initiating session completion `{sid}`...");

        let uid = sid.user_id as u64;

        // Если это приватный/тестовый пользователь (uid == 0) — стираем полностью
        if uid == 0 {
            info!("[Session] Ephemeral user detected (uid=0). Purging session files.");
            return Self::remove(sid).await;
        }

        // Remove session at user state
        let (removed_session, is_empty) = {
            if let Ok(user) = UserState::get_or_init(uid).await {
                let mut user_guard = user.write().await;

                let session = user_guard.sessions.remove(sid);
                let empty = user_guard.sessions.is_empty();

                (session, empty)
            } else {
                (None, false)
            }
        };

        // Flush unsaved changes to database
        if let Some(session) = removed_session {
            let session_guard = session.lock().await;

            let table_name = str!(sid);
            let table = session_guard.kv_db.open_table(&table_name).await?;
            table.flush().await?;
        }

        // If no more active sessions - clean up user's state from memory
        if is_empty {
            if let Ok(user) = UserState::get_or_init(uid).await {
                let user_guard = user.read().await;
                if user_guard.sessions.is_empty() {
                    drop(user_guard);
                    USER_STATES.remove(&uid).await;
                }
            }
        }

        info!("[Session] Session finished successfully.");

        Ok(())
    }

    /// Completely removes user session from memory AND disk.
    #[log(sid = %sid)]
    pub async fn remove(sid: &SessionId) -> Result<()> {
        info!("[Session] Initiating full session removal `{sid}`...");

        let uid = sid.user_id as u64;

        // 1. Удаляем сессию из UserState в памяти и чистим её ID из метаданных
        let (removed_session, is_empty) = {
            if let Ok(user) = UserState::get_or_init(uid).await {
                let mut user_guard = user.write().await;

                let session = user_guard.sessions.remove(sid);
                let empty = user_guard.sessions.is_empty();

                if let Ok(Some(mut user_meta)) = user_guard.load_metadata().await {
                    user_meta.sessions.retain(|s| s != sid);
                    if user_meta.last_session == Some(*sid) {
                        user_meta.last_session = user_guard.sessions.keys().next().copied();
                    }
                    user_guard.save_metadata(&user_meta).await.ok();
                }

                (session, empty)
            } else {
                (None, false)
            }
        };

        // 2. Флашим таблицы сессии и сбрасываем Lock/Guard перед удалением файлов
        if let Some(session) = removed_session {
            let session_guard = session.lock().await;
            let table_name = str!(sid);
            if let Ok(table) = session_guard.kv_db.open_table(&table_name).await {
                table.flush().await.ok();
            }
            drop(session_guard);
        }

        // 3. Удаляем ИСКЛЮЧИТЕЛЬНО папку этой конкретной сессии
        let session_dir = path!("$share$/users/{uid}/sessions/{sid}");
        if atoman::fs::metadata(&session_dir).await.is_ok() {
            if let Err(e) = atoman::fs::remove_dir_all(&session_dir).await {
                error!("[Session] Failed to remove session directory `{session_dir:?}`: {e}");
            }
        }

        // 4. Освобождаем UserState из RAM, если активных сессий в памяти не осталось
        // Корневую директорию пользователя ($share$/users/{uid}) НЕ ТРОГАЕМ.
        if is_empty {
            if let Ok(user) = UserState::get_or_init(uid).await {
                let user_guard = user.read().await;
                if user_guard.sessions.is_empty() {
                    drop(user_guard);
                    USER_STATES.remove(&uid).await;
                }
            }
        }

        info!("[Session] Session `{sid}` removed completely.");

        Ok(())
    }
}

impl Session {
    /// Reads session metadata.
    #[log(sid = %self.id)]
    pub async fn read_metadata(&self) -> Result<Option<SessionMetadata>> {
        info!("[Session] Reading session metadata...");

        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        table.read(Key::Metadata).await
    }

    /// Reads session messages.
    #[log(sid = %self.id)]
    pub async fn read_messages(&self) -> Result<Vec<Message>> {
        info!("[Session] Reading session messages...");

        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let meta = match table.read(Key::Metadata).await? {
            Some(meta) => meta,
            None => {
                let new_meta = SessionMetadata {
                    session_id: self.id,
                    ..Default::default()
                };
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

    /// Writes new message to session.
    #[log(sid = %self.id)]
    pub async fn write_message(&self, message: Message) -> Result<()> {
        info!("[Session] Writing new session message...");

        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let mut meta: SessionMetadata =
            table.read(Key::Metadata).await?.unwrap_or(SessionMetadata {
                session_id: self.id,
                ..Default::default()
            });

        let msg_key = Key::Message(meta.message_count as usize);
        table.write(msg_key, message).await?;
        meta.message_count += 1;

        table.write(Key::Metadata, meta).await?;
        table.flush().await?;

        Ok(())
    }

    /// Writes new messages to sessions (multiple).
    #[log(sid = %self.id)]
    pub async fn write_messages(&self, messages: Vec<Message>) -> Result<()> {
        info!("[Session] Writing new session messages...");

        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        let mut meta: SessionMetadata =
            table.read(Key::Metadata).await?.unwrap_or(SessionMetadata {
                session_id: self.id,
                ..Default::default()
            });

        for message in messages {
            let msg_key = Key::Message(meta.message_count as usize);
            table.write(msg_key, message).await?;
            meta.message_count += 1;
        }

        table.write(Key::Metadata, meta).await?;
        table.flush().await?;

        Ok(())
    }

    /// Insert compressed message to session.
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
            .unwrap_or(SessionMetadata {
                session_id: self.id,
                ..Default::default()
            });

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

        let new_message_count = std::cmp::max(current_meta.message_count, current_idx);
        let new_meta = SessionMetadata {
            session_id: self.id,
            message_count: new_message_count,
            compressed_until: insert_idx,
        };

        table.write(Key::Metadata, new_meta).await?;
        table.flush().await?;

        Ok(())
    }

    /// Clears session data.
    #[log(sid = %self.id)]
    pub async fn clear(&self) -> Result<()> {
        info!("[Session] Clearing message history...");

        let table_name = str!(self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        if let Some(meta) = table.read::<_, SessionMetadata>(Key::Metadata).await? {
            for i in 0..meta.message_count {
                table.remove(Key::Message(i)).await?;
            }
        }

        let fresh_meta = SessionMetadata {
            session_id: self.id,
            ..Default::default()
        };

        table.write(Key::Metadata, fresh_meta).await?;
        table.flush().await?;

        info!("[Session] Message history has been cleared.");

        Ok(())
    }
}

impl Session {
    /// Saves user rule to session (local or global).
    pub async fn save_rule(
        &self,
        id: Option<u64>,
        text: String,
        is_global: bool,
    ) -> Result<UserRule> {
        if is_global {
            let user = UserState::get_or_init(self.user_id).await?;
            let user_guard = user.read().await;

            user_guard.save_rule(id, text).await
        } else {
            let rule_id = id.unwrap_or_else(gen_id);
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

    /// Reads user rule by ID.
    pub async fn read_rule(&self, id: u64) -> Result<Option<UserRule>> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Some(rule) = local_table.read::<_, UserRule>(id).await? {
            return Ok(Some(rule));
        }

        let user = UserState::get_or_init(self.user_id).await?;
        let user_guard = user.read().await;

        let global_table = user_guard.kv_db.open_table(RULES_TABLE_NAME).await?;
        global_table.read::<_, UserRule>(id).await
    }

    /// Removes user rule by ID.
    pub async fn remove_rule(&self, id: u64) -> Result<bool> {
        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if local_table.read::<_, UserRule>(id).await?.is_some() {
            local_table.remove(id).await?;
            local_table.flush().await?;
            return Ok(true);
        }

        let user = UserState::get_or_init(self.user_id).await?;
        let user_guard = user.read().await;

        user_guard.remove_rule(id).await
    }

    /// Loads all user's rules (global + local).
    pub async fn load_rules(&self) -> Result<Vec<UserRule>> {
        let user = UserState::get_or_init(self.user_id).await?;
        let user_guard = user.read().await;

        let mut rules = user_guard.load_rules().await?;

        let local_table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if let Ok(local_rules) = local_table.read_all::<u64, UserRule>().await {
            rules.extend(local_rules.into_iter().map(|r| r.1));
        }

        Ok(rules)
    }

    /// Removes all local session's rules.
    pub async fn clear_rules(&self) -> Result<()> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        table.clear().await?;
        table.flush().await?;

        Ok(())
    }

    /// Removes all user's rules.
    pub async fn clear_all_rules(&self) -> Result<()> {
        let user = UserState::get_or_init(self.user_id).await?;
        let user_guard = user.read().await;

        user_guard.clear_rules().await
    }
}

impl Session {
    /// Duplicates session with all messages & local rules.
    #[log(sid = %self.id)]
    pub async fn duplicate(&self) -> Result<SessionId> {
        info!("[Session] Cloning session...");

        let new_id = SessionId::new(self.id.user_id);

        let user_base = path!("$share$/users/{}", new_id.user_id);
        let new_session_dir = user_base.join("sessions").join(new_id.to_string());
        let new_kv_db = Arc::new(Storage::connect(new_session_dir).await?);

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
            let user = UserState::get_or_init(self.user_id).await?;
            let mut user_guard = user.write().await;

            let mut user_meta = user_guard.load_metadata().await?.unwrap_or_default();

            if !user_meta.sessions.contains(&new_id) {
                user_meta.sessions.push(new_id);
            }
            user_meta.last_session = Some(new_id);
            user_guard.save_metadata(&user_meta).await?;

            user_guard.sessions.insert(new_id, new_session);
        }

        info!("[Session] Session cloned to `{new_id}`.");

        Ok(new_id)
    }
}
