//! Agents management module.

pub mod agent;
pub use agent::Agent;

use crate::{prelude::*, skills};

use anylm::api::Tool;
use osy_share::AgentMeta;
use std::fmt::Write;
use tokio::task::JoinSet;

/// Agents manager state.
pub static MANAGER: State<Manager> = State::default();

/// Agents manager.
#[derive(Default, Clone)]
pub struct Manager {
    pub agents: Arc<SharedMap<String, Agent>>,
    pub agents_doc: Arc<String>,
    pub tools: Arc<Vec<Tool>>,
}

impl Manager {
    /// Initializes & runs the agents management.
    #[log(skip_all)]
    pub async fn init() -> Result<()> {
        let scan_dir = path!("$/");

        // check scan dir
        if !scan_dir.exists() {
            warn!("[Manager] Core directory not found at: {scan_dir:?}.");
            return Ok(());
        }

        let mut set = JoinSet::new();
        let mut reader = Dir::read(scan_dir).await?;

        info!("[Manager] Scanning for agent binaries...");

        // read files in core dir
        while let Some(entry) = reader.next_file().await? {
            let path = entry.path().clone();
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // check if it's an agent binary
            if file_name.starts_with("osy-") && !Self::has_agent_path(&path).await {
                // spawn agent running:
                set.spawn(async move { Agent::run(path).await });
            }
        }

        // check results
        while let Some(task_res) = set.join_next().await {
            if let Err(e) = task_res {
                error!("[Manager] Agent startup task panicked: {e}");
            }
        }

        // gen task delegation tool
        Self::gen_basic_tools().await;

        Ok(())
    }

    /// Generates & sets the basic tools schemes.
    pub async fn gen_basic_tools() {
        let tools = vec![
            skills::eval::tools_list(),
            skills::task::tools_list(),
            skills::fact::tools_list(),
        ]
        .into_iter()
        .flatten()
        .collect();

        MANAGER.lock().await.tools = arc!(tools);
    }

    /// Checks & updates agents list.
    #[log(skip_all)]
    pub async fn update() -> Result<()> {
        info!("[Manager] Starting agents update cycle...");

        // collect the list of all the outdated agents:
        let mut to_restart = Vec::new();
        {
            let guard = MANAGER.get();
            for (_name, agent) in guard.agents.to_hash().await {
                if agent.read().await.check(true).await.unwrap_or(true) {
                    to_restart.push(agent);
                }
            }
        }

        // stop all the outdated agents:
        for agent in to_restart {
            let agent = agent.read().await;
            let name = &agent.metadata.name;

            warn!("[Manager] Agent `{name}` is outdated...");
            agent.stop().await?;
        }

        Self::init().await?;

        info!("[Manager] Agents update cycle completed.");
        Ok(())
    }

    /// Generates agents documentation.
    pub(super) async fn gen_doc() {
        let guard = MANAGER.get();

        // gen message, if agents not found
        if guard.agents.is_empty().await {
            MANAGER.lock().await.agents_doc = arc!("No active skills available.".to_string());
            return;
        }

        // gen skills doc:
        let mut doc_builder = String::from("Available Skills:\n");
        for (_, agent) in guard.agents.to_hash().await {
            for (_, skill) in &agent.read().await.metadata.skills {
                let _ = writeln!(
                    doc_builder,
                    "* `{}`: {}",
                    skill.name,
                    skill.description.trim().replace("\n", "")
                );
            }
        }

        MANAGER.lock().await.agents_doc = arc!(doc_builder);
        info!(
            "[Manager] Documentation updated ({} agents processed).",
            guard.agents.count().await
        );
    }

    /// Returns agents documentation for prompt.
    pub async fn agents_doc() -> Arc<String> {
        MANAGER.get().agents_doc.clone()
    }

    /// Returns basic tools list.
    pub async fn basic_tools() -> Vec<Tool> {
        (*MANAGER.get().tools).clone()
    }

    /// Returns agents list.
    pub async fn agents_list() -> Vec<AgentMeta> {
        let mut agents = vec![];

        for (_, agent) in MANAGER.get().agents.to_hash().await {
            let guard = agent.read().await;
            agents.push(AgentMeta {
                name: guard.metadata.name.clone(),
                description: guard.metadata.description.clone(),
                ..Default::default()
            });
        }

        agents
    }

    /// Returns true if agent with this name on running.
    pub async fn has_agent(name: &Arc<String>) -> bool {
        MANAGER.get().agents.read(name).await.is_some()
    }

    /// Returns true if agent with this path on running.
    pub async fn has_agent_path(path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        MANAGER
            .get()
            .agents
            .find(|_, agent| async move { agent.exec_path == path })
            .await
            .is_some()
    }

    /// Returns agent reference by name.
    pub async fn get_agent(name: &String) -> Option<SharedItem<Agent>> {
        MANAGER
            .get()
            .agents
            .get(name)
            .await
            .map(|agent| agent.clone())
    }

    /// Returns agent reference by skill name.
    pub async fn get_by_skill(skill: &str) -> Option<SharedItem<Agent>> {
        if let Some((_, agent)) = MANAGER
            .get()
            .agents
            .find(|_, agent| async move { agent.metadata.skills.contains_key(skill) })
            .await
        {
            Some(agent.clone())
        } else {
            None
        }
    }
}
