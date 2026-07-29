use super::Task;
use crate::{prelude::*, session::Session};

use anylm::api::{Content, Messages};
use tokio::task::JoinHandle;

/// The agent tasks worflow
pub struct Tasks {
    pub pending: HashMap<i64, Task>,
    pub working: HashMap<i64, Arc<JoinHandle<()>>>,
    pub finished: HashSet<i64>,
    pub results: HashMap<i64, Vec<Content>>,
    pub session: Arc<Mutex<Session>>,
    pub messages: Arc<Mutex<Messages>>,
}

impl Tasks {
    /// Creates a new workflow
    pub fn new(session: Arc<Mutex<Session>>, messages: Arc<Mutex<Messages>>) -> Arc<Mutex<Self>> {
        arc!(Mutex::new(Self {
            pending: map! {},
            working: map! {},
            finished: set![],
            results: map! {},
            session,
            messages,
        }))
    }

    /// Returns true if task ready to start
    pub fn check(&self, task: &Task) -> bool {
        task.depends.is_empty() || task.depends.iter().all(|id| self.finished.contains(id))
    }

    /// Returns true if task result is required
    pub fn is_result_needed(&self, task_id: i64) -> bool {
        self.pending
            .values()
            .any(|task| task.depends.contains(&task_id))
    }
}
