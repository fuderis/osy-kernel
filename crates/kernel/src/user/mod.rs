pub mod session;
pub use session::Session;

pub mod key;
pub use key::Key;

pub mod metadata;
pub use metadata::{SessionMetadata, UserMetadata};

use std::collections::HashMap;
use std::sync::Arc;

use crate::prelude::*;

use atoman::{Map, MapGuard, MapGuardMut};
use cistern::{Cistern, Kv, Rag, RagRecord, generate_id};
use osy_share::{SessionId, UserFact, UserRule};

/// Глобальное шардированное хранилище состояний пользователей.
/// Полностью асинхронное, без синхронных блокировок.
static USER_STATES: Map<u64, UserState> = Map::new();

const FACTS_TABLE_NAME: &str = "facts";
const RULES_TABLE_NAME: &str = "rules";

/// Объединенные базы данных и активные сессии одного пользователя.
pub struct UserState {
    /// Уникальный идентификатор пользователя.
    pub user_id: u64,
    /// KV-база данных (Cistern) для метаданных и правил.
    pub kv_db: Arc<Cistern<Kv>>,
    /// Векторная база данных (RAG) для фактов.
    pub rag_db: Arc<Cistern<Rag>>,
    /// Карта активных сессий пользователя в памяти.
    pub sessions: HashMap<SessionId, session::SharedSession>,
}

impl UserState {
    /// Возвращает асинхронный RAII-гард для чтения состояния пользователя.
    /// Если пользователя нет в памяти, инициализирует его с диска.
    pub async fn get_or_init_read(user_id: u64) -> Result<MapGuard<UserState>> {
        if let Some(guard) = USER_STATES.read(&user_id).await {
            return Ok(guard);
        }

        // Инициализируем через write-путь и сразу сбрасываем W-Lock
        {
            let _write_guard = Self::get_or_init(user_id).await?;
        } // _write_guard дропается здесь

        USER_STATES
            .read(&user_id)
            .await
            .ok_or_else(|| Error::Custom("Failed to acquire user state read guard".into()).into())
    }

    /// Возвращает асинхронный RAII-гард для записи в состояние пользователя.
    /// Если пользователя нет в памяти, инициализирует его и подключает базы с диска.
    pub async fn get_or_init(user_id: u64) -> Result<MapGuardMut<UserState>> {
        // 1. Быстрый путь: пробуем захватить существующее состояние на запись
        if let Some(guard) = USER_STATES.write(&user_id).await {
            return Ok(guard);
        }

        // 2. Медленный путь: инициализируем БД с диска асинхронно
        let user_base = path!("$share$/users/{user_id}");
        let kv_dir = user_base.join("userdata");
        let kv_db = Arc::new(Cistern::connect(kv_dir).await?);

        let context_dir = user_base.join("context");
        let rag_db = Arc::new(Cistern::connect(context_dir).await?);

        let state = Self {
            user_id,
            kv_db,
            rag_db,
            sessions: HashMap::new(),
        };

        // 3. Асинхронно вставляем созданное состояние
        USER_STATES.insert(user_id, state).await;

        // 4. Захватываем write-гард гарантированно без blocking-накладок
        USER_STATES.write(&user_id).await.ok_or_else(|| {
            Error::Custom("Failed to acquire user state lock after initialization".into()).into()
        })
    }

    /// Возвращает список ID сессий пользователя, упорядоченный по свежести.
    pub async fn sessions_list(user_id: u64, limit: usize) -> Result<Vec<SessionId>> {
        // Достаточно R-Lock (get_or_init_read), так как мы не модифицируем структуру UserState
        let guard = Self::get_or_init_read(user_id).await?;

        let meta = guard.get_metadata().await?.unwrap_or_default();
        let mut sessions = meta.sessions;

        sessions.reverse();
        if limit > 0 && sessions.len() > limit {
            sessions.truncate(limit);
        }

        Ok(sessions)
    }

    // --- Metadata API ---

    /// Загружает глобальные метаданные пользователя.
    pub async fn get_metadata(&self) -> Result<Option<UserMetadata>> {
        let table = self.kv_db.open_table("global").await?;
        table.read("user_metadata").await
    }

    /// Сохраняет глобальные метаданные пользователя на диск.
    pub async fn save_metadata(&self, meta: &UserMetadata) -> Result<()> {
        let table = self.kv_db.open_table("global").await?;
        table.write("user_metadata", meta.clone()).await?;
        table.flush().await?;
        Ok(())
    }

    // --- Global Rules API ---

    /// Возвращает список всех глобальных правил пользователя.
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

    /// Создает или обновляет глобальное правило.
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

    /// Удаляет глобальное правило по идентификатору.
    pub async fn remove_global_rule(&self, id: u64) -> Result<bool> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        if table.read::<_, UserRule>(id).await?.is_some() {
            table.remove(id).await?;
            table.flush().await?;
            return Ok(true);
        }
        Ok(false)
    }

    /// Очищает все глобальные правила пользователя.
    pub async fn clear_global_rules(&self) -> Result<()> {
        let table = self.kv_db.open_table(RULES_TABLE_NAME).await?;
        table.clear().await?;
        table.flush().await?;
        Ok(())
    }

    // --- Facts / RAG API ---

    /// Сохраняет факт с дедупликацией по схожести эмбеддингов.
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

    /// Выполняет векторный поиск фактов пользователя по запросу.
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

    /// Возвращает все сохраненные факты пользователя.
    pub async fn list_all_facts(&self) -> Result<Vec<UserFact>> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        let records = table.read_all().await?;
        Ok(records
            .unwrap_or_default()
            .into_iter()
            .map(|f| f.data)
            .collect())
    }

    /// Удаляет факт по идентификатору.
    pub async fn remove_fact(&self, fact_id: u64) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.remove(fact_id).await?;
        Ok(())
    }

    /// Полностью очищает векторную базу данных фактов пользователя.
    pub async fn clear_all_facts(&self) -> Result<()> {
        let table = self.rag_db.open_table(FACTS_TABLE_NAME).await?;
        table.clear().await?;
        Ok(())
    }
}
