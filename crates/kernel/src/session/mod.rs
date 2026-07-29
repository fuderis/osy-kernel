pub mod key;
use key::Key;

pub mod metadata;
use metadata::Metadata;

use crate::{context::UserFact, prelude::*};

use anylm::api::Message;
use cistern::{Cistern, Kv, Rag};
use ovsy_share::{SessionId, SessionInfo};

static SESSIONS: State<HashMap<SessionId, SharedSession>> = State::default();

type SharedSession = Arc<Mutex<Session>>;

/// The user session manager
#[derive(Clone)]
pub struct Session {
    /// Unique identifier of the active session
    pub id: SessionId,
    /// Runtime environment info and metadata for the session
    pub info: SessionInfo,
    /// Key-Value storage instance for session chat history and metadata
    pub kv_db: Arc<Cistern<Kv>>,
    /// Vector RAG database instance for user-specific long-term memory/facts
    pub rag_db: Arc<Cistern<Rag>>,
}

impl Session {
    /// Initializes the user session instance
    pub async fn init(id: SessionId, info: SessionInfo) -> Result<SharedSession> {
        let user_dir = path!("$share$/userdata/{}", id.user_id);

        // session kv database path
        let session_dir = user_dir.join("sessions").join(id.to_string());
        let kv_db = arc!(Cistern::connect(session_dir).await?);

        // global user rag database path
        let facts_dir = user_dir.join("facts");
        let rag_db = arc!(Cistern::connect(facts_dir).await?);

        let this = arc_mutex!(Self {
            id,
            info,
            kv_db,
            rag_db,
        });

        SESSIONS.lock().await.insert(id, this.clone());
        Ok(this)
    }

    /// Returns the user session instance
    pub fn get(id: &SessionId) -> Option<SharedSession> {
        SESSIONS.dirty_get().get(id).map(Clone::clone)
    }

    /// Finishes the user session
    pub async fn finish(id: &SessionId) -> Result<()> {
        if let Some(session) = SESSIONS.lock().await.remove(id) {
            let table_name = Self::table_name(id);
            let table = session.lock().await.kv_db.open_table(&table_name).await?;
            table.flush().await?;
        }
        Ok(())
    }

    /// Reads the session metadata
    pub async fn read_metadata(&self) -> Result<Option<Metadata>> {
        let table_name = Self::table_name(&self.id);
        let table = self.kv_db.open_table(&table_name).await?;

        table.read(Key::Metadata).await
    }

    /// Reads all the session messages
    pub async fn read_messages(&self) -> Result<Vec<Message>> {
        let table_name = Self::table_name(&self.id);
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

    /// Writes a new message to the session
    pub async fn write_message(&self, message: Message) -> Result<()> {
        let table_name = Self::table_name(&self.id);
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

    /// Writes new messages to the session
    pub async fn write_messages(&self, messages: Vec<Message>) -> Result<()> {
        let table_name = Self::table_name(&self.id);
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

    /// Inserts a message after the compressed originals and shifts the preserve messages
    pub async fn insert_and_shift(
        &self,
        compressed_msg: Message,
        preserve_msgs: Vec<Message>,
        compress_count: usize,
    ) -> Result<()> {
        let table_name = Self::table_name(&self.id);
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

    /// Completely clears the session message history
    pub async fn clear(&self) -> Result<()> {
        let table_name = Self::table_name(&self.id);
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

    fn table_name(session_id: &SessionId) -> String {
        str!("{session_id}")
    }
}

// Global user RAG methods
impl Session {
    fn facts_table_name() -> String {
        "facts".into()
    }

    /// Saves a new user fact while removing previous similar entries
    pub async fn save_fact(&self, embedding: Vec<f32>, text: String) -> Result<()> {
        let table_name = Self::facts_table_name();
        let table = self.rag_db.open_table(&table_name).await?;
        let dedup_threshold = Settings::get().context.dedup_similarity;

        // search for existing close duplicates
        if let Ok(Some(similar_facts)) = table
            .read::<UserFact>(embedding.clone(), 5, dedup_threshold)
            .await
        {
            for record in similar_facts {
                // skip saving if exact duplicate exists
                if record.data.text.trim().eq_ignore_ascii_case(text.trim()) {
                    return Ok(());
                }

                // remove stale or outdated fact before replacing
                let _ = table.remove(record.id).await;
            }
        }

        // write new fact
        let fact = UserFact {
            text,
            created_at: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
        };

        table.write(embedding, fact).await?;
        Ok(())
    }

    /// Searches for relevant user facts across all sessions
    pub async fn search_facts(
        &self,
        query_embedding: Vec<f32>,
        limit: usize,
        distance_threshold: f32,
    ) -> Result<Vec<cistern::RagRecord<UserFact>>> {
        let table_name = Self::facts_table_name();
        let table = self.rag_db.open_table(&table_name).await?;

        let records = table
            .read(query_embedding, limit, distance_threshold)
            .await?;

        Ok(records.unwrap_or_default())
    }

    /// Removes a fact by its ID
    pub async fn remove_fact(&self, fact_id: u64) -> Result<()> {
        let table_name = Self::facts_table_name();
        let table = self.rag_db.open_table(&table_name).await?;

        table.remove(fact_id).await?;
        Ok(())
    }
}
