pub mod agent;
pub use agent::Agent;

use crate::{prelude::*, skills};

use anylm::api::Tool;
use osy_share::AgentMetadata;
use std::fmt::Write;
use tokio::task::JoinSet;

/// The agents manager state
pub static MANAGER: State<Manager> = State::default();

/// The agents manager
#[derive(Default, Debug, Clone)]
pub struct Manager {
    pub agents: HashMap<Arc<String>, Arc<Agent>>,
    pub agents_doc: Arc<String>,
    pub tools: Arc<Vec<Tool>>,
}

impl Manager {
    /// Initializes & runs the agents management
    pub async fn init() -> Result<()> {
        let scan_dir = path!("$/");

        // check scan dir:
        if !scan_dir.exists() {
            warn!("[Manager] Core directory not found at: {scan_dir:?}.");
            return Ok(());
        }

        let mut set = JoinSet::new();
        let mut reader = Dir::read(scan_dir).await?;

        info!("[Manager] Scanning for agent binaries...");

        // read files in core dir:
        while let Some(entry) = reader.next_file().await? {
            let path = entry.path().clone();
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // check if it's an agent binary (starts with "osy-")
            if file_name.starts_with("osy-") && !Self::contains_path(&path).await {
                // spawn agent running:
                set.spawn(async move { Self::run(path).await });
            }
        }

        // check results:
        while let Some(task_res) = set.join_next().await {
            if let Err(e) = task_res {
                error!("[Manager] Agent startup task panicked: {e}");
            }
        }

        // gen task delegation tool:
        Self::gen_basic_tools().await;

        Ok(())
    }

    /// Generates & sets the basic tools schemes
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

    /// Ensures the agent is running and healthy, spawning it if necessary
    pub async fn ensure_agent(name: &Arc<String>) -> Result<Option<PathBuf>> {
        let needs_start = {
            let guard = MANAGER.get().await;
            if let Some(agent) = guard.agents.get(name) {
                agent.check().await.unwrap_or(true)
            } else {
                true
            }
        };

        if needs_start {
            let agent_bin = path!("$/").join(&str!("osy-{}", name.as_str()));
            if !agent_bin.exists() {
                warn!("[Manager] Agent `{name}` requested but binary not found at {agent_bin:?}.");
                return Ok(None);
            }

            info!("[Manager] Agent `{name}` is missing or unresponsive. Attempting to start...");

            let _ = Self::stop(name.clone()).await;

            if let Err(e) = Self::run(agent_bin).await {
                error!("[Manager] Failed to recover agent `{name}`: {e}");
                return Ok(None);
            }
        }

        Ok(Self::agent_options(name).await)
    }

    /// Runs the AI agent server
    pub async fn run(bin_path: impl Into<PathBuf>) -> Result<()> {
        let path: PathBuf = bin_path.into();
        info!("[Manager] Starting agent {:?}...", path.display());

        if let Some(agent) = Agent::run(path.clone()).await? {
            let name = arc!(agent.metadata.name.clone());
            let mut lock = MANAGER.lock().await;

            if !lock.agents.contains_key(&name) {
                lock.agents.insert(name.clone(), arc!(agent));
                info!("[Manager] Agent `{name}` added to manager.");
            } else {
                warn!("[Manager] Agent `{name}` is already running, skipping...");
            }
        }

        Self::update_doc().await?;
        Ok(())
    }

    /// Stops the AI agent server
    pub async fn stop(name: Arc<String>) -> Result<()> {
        let mut lock = MANAGER.lock().await;
        if lock.agents.remove(&name).is_some() {
            info!("[Manager] Agent `{name}` stopped and removed.");
        } else {
            warn!("[Manager] Attempted to stop unknown `{name}` agent.");
        }

        Self::update_doc().await?;
        Ok(())
    }

    /// Updates the AI agents list
    pub async fn update() -> Result<()> {
        info!("[Manager] Starting agents update cycle...");

        // collect the list of all the outdated agents:
        let mut to_restart = Vec::new();
        {
            let guard = MANAGER.get().await;
            for (name, agent) in &guard.agents {
                if agent.check().await? {
                    to_restart.push(name.clone());
                }
            }
        }

        // stop all the outdated agents:
        for name in to_restart {
            warn!("[Manager] Agent `{}` needs update, stopping...", name);
            Self::stop(name).await?;
        }

        Self::init().await?;

        info!("[Manager] Agents update cycle completed.");
        Ok(())
    }

    /// Updates the agents list prompt part
    pub async fn update_doc() -> Result<()> {
        let guard = MANAGER.get().await;

        // gen message, if agents not found:
        if guard.agents.is_empty() {
            MANAGER.lock().await.agents_doc = arc!("No active skills available.".to_string());
            return Ok(());
        }

        // gen skills doc:
        let mut doc_builder = String::from("Available Skills:\n");
        for agent in guard.agents.values() {
            for skill in &agent.metadata.skills {
                let _ = writeln!(
                    doc_builder,
                    "* `{}.{}`: {}",
                    agent.metadata.name,
                    skill.name,
                    skill.description.trim().replace("\n", "")
                );
            }
        }

        MANAGER.lock().await.agents_doc = arc!(doc_builder);
        info!(
            "[Manager] Documentation updated ({} agents processed).",
            guard.agents.len()
        );
        Ok(())
    }

    /// Returns the all agents list
    pub async fn agents_list() -> Vec<AgentMetadata> {
        MANAGER
            .get()
            .await
            .agents
            .iter()
            .map(|(_, agent)| AgentMetadata {
                name: agent.metadata.name.clone(),
                description: agent.metadata.description.clone(),
                ..Default::default()
            })
            .collect()
    }

    /// Returns true if agent with this name is already on running
    pub async fn contains(name: &Arc<String>) -> bool {
        MANAGER.get().await.agents.contains_key(name)
    }

    /// Returns true if agent with this name is already on running
    pub async fn contains_path(path: impl AsRef<Path>) -> bool {
        let path = path.as_ref();
        MANAGER
            .get()
            .await
            .agents
            .iter()
            .find(|(_, agent)| &agent.exec_path == path)
            .is_some()
    }

    /// Returns the agents list prompt part
    pub async fn agents_list_doc() -> Arc<String> {
        MANAGER.get().await.agents_doc.clone()
    }

    /// Returns the bsic tools list
    pub async fn basic_tools() -> Vec<Tool> {
        (*MANAGER.get().await.tools).clone()
    }

    /// Returns the agent system prompt
    pub async fn agent_prompt(name: &Arc<String>, skill: &str) -> Option<String> {
        MANAGER.get().await.agents.get(name).map(|agent| {
            agent
                .metadata
                .skills
                .iter()
                .find(|s| &s.name == skill)
                .map(|s| s.prompt.clone())
        })?
    }

    /// Returns the agent tools list
    pub async fn agent_tools(name: &Arc<String>, skill: &str) -> Result<Option<Vec<Tool>>> {
        let mngr = MANAGER.get().await;
        let Some(agent) = mngr.agents.get(name) else {
            return Ok(None);
        };

        let client = Client::ipc(&agent.sock_path.to_string_lossy());

        let request = client.post(&str!("/skills/{skill}/tools"));
        let tools = request.send().await?.json().await?;

        Ok(Some(tools))
    }

    /// Returns the agent options (port, prompt, tools)
    pub async fn agent_options(name: &Arc<String>) -> Option<PathBuf> {
        MANAGER
            .get()
            .await
            .agents
            .get(name)
            .map(|agent| agent.sock_path.clone())
    }
}
