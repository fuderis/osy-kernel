pub mod agent;
pub use agent::Agent;

pub mod tasks;
pub use tasks::Tasks;

pub mod task;
pub use task::Task;

use crate::{prelude::*, skills};

use anylm::api::Tool;
use ovsy_share::{AgentMetadata, Skill};
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
            warn!("Core directory not found at: {scan_dir:?}");
            return Ok(());
        }

        let mut set = JoinSet::new();
        let mut reader = Dir::read(scan_dir).await?;

        info!("Scanning for agent binaries...");

        // read files in core dir:
        while let Some(entry) = reader.next_file().await? {
            let path = entry.path().clone();
            let file_name = path
                .file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .to_string();

            // check if it's an agent binary (starts with "ovsy-")
            if file_name.starts_with("ovsy-") {
                // spawn agent running:
                set.spawn(async move { Self::run(path).await });
            }
        }

        // check results:
        while let Some(task_res) = set.join_next().await {
            if let Err(e) = task_res {
                error!("Agent startup task panicked: {e}");
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
    pub async fn ensure_agent(name: &Arc<String>) -> Result<Option<(PathBuf, String, Vec<Skill>)>> {
        let needs_start = {
            let guard = MANAGER.get().await;
            if let Some(agent) = guard.agents.get(name) {
                agent.check().await.unwrap_or(true)
            } else {
                true
            }
        };

        if needs_start {
            let binary_name = format!("ovsy-{name}",);
            let agent_bin = path!("$/").join(binary_name);

            if !agent_bin.exists() {
                warn!("Agent `{name}` requested but binary not found at {agent_bin:?}");
                return Ok(None);
            }

            info!("Agent `{name}` is missing or unresponsive. Attempting to start...");

            let _ = Self::stop(name.clone()).await;

            if let Err(e) = Self::run(agent_bin).await {
                error!("Failed to recover agent `{name}`: {e}");
                return Ok(None);
            }
        }

        Ok(Self::agent_options(name).await)
    }

    /// Runs the AI agent server
    pub async fn run(bin_path: impl Into<PathBuf>) -> Result<()> {
        let path: PathBuf = bin_path.into();
        info!("Starting agent {:?}...", path.display());

        if let Some(agent) = Agent::run(path.clone()).await? {
            let name = arc!(agent.metadata.name.clone());
            let mut lock = MANAGER.lock().await;

            if !lock.agents.contains_key(&name) {
                lock.agents.insert(name.clone(), arc!(agent));
                info!("Agent `{name}` added to manager");
            } else {
                warn!("Agent `{name}` is already running, skipping");
            }
        }

        Self::update_doc().await?;
        Ok(())
    }

    /// Stops the AI agent server
    pub async fn stop(name: Arc<String>) -> Result<()> {
        let mut lock = MANAGER.lock().await;
        if lock.agents.remove(&name).is_some() {
            info!("Agent `{name}` stopped and removed");
        } else {
            warn!("Attempted to stop unknown `{name}` agent");
        }

        Self::update_doc().await?;
        Ok(())
    }

    /// Updates the AI agents list
    pub async fn update() -> Result<()> {
        info!("Starting agents update cycle...");

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
            warn!("Agent `{}` needs update, restarting...", name);
            Self::stop(name).await?;
        }

        Self::init().await?;
        info!("Agents update cycle completed");
        Ok(())
    }

    /// Updates the agents list prompt part
    pub async fn update_doc() -> Result<()> {
        let guard = MANAGER.get().await;

        // gen message, if agents not found:
        if guard.agents.is_empty() {
            MANAGER.lock().await.agents_doc = arc!("No active agents available.".to_string());
            return Ok(());
        }

        // gen agents doc:
        let mut doc_builder = String::from("Available Agents:\n");
        for agent in guard.agents.values() {
            let _ = writeln!(
                doc_builder,
                "* Agent `{}`: \n  Description: \"{}\"\n  Skills: {}",
                agent.metadata.name,
                agent.metadata.description.trim().replace("\n", ""),
                agent
                    .metadata
                    .skills
                    .iter()
                    .map(|s| str!("    * `{}`: {}", s.name, s.description))
                    .collect::<Vec<_>>()
                    .join("\n")
            );
        }

        MANAGER.lock().await.agents_doc = arc!(doc_builder);
        info!(
            "Documentation updated ({} agents listed)",
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

    /// Returns the agents list prompt part
    pub async fn agents_list_doc() -> Arc<String> {
        MANAGER.get().await.agents_doc.clone()
    }

    /// Returns the bsic tools list
    pub async fn basic_tools() -> Vec<Tool> {
        (*MANAGER.get().await.tools).clone()
    }

    /// Returns the agent system prompt
    pub async fn agent_prompt(name: &Arc<String>) -> Option<String> {
        MANAGER
            .get()
            .await
            .agents
            .get(name)
            .map(|agent| agent.metadata.prompt.clone())
    }

    /// Returns the agent tools list
    pub async fn agent_tools(name: &Arc<String>, skill: Option<&str>) -> Result<Option<Vec<Tool>>> {
        let mngr = MANAGER.get().await;
        let Some(agent) = mngr.agents.get(name) else {
            return Ok(None);
        };

        let client = Client::ipc(&agent.sock_path.to_string_lossy());

        let mut request = client.post("/tools/list");
        if let Some(skill) = skill {
            request = request.json(&json!({ "skill": skill }));
        }

        let tools = request.send().await?.json().await?;

        Ok(Some(tools))
    }

    /// Returns the agent options (port, prompt, tools)
    pub async fn agent_options(name: &Arc<String>) -> Option<(PathBuf, String, Vec<Skill>)> {
        MANAGER.get().await.agents.get(name).map(|agent| {
            (
                agent.sock_path.clone(),
                agent.metadata.prompt.clone(),
                agent.metadata.skills.clone(),
            )
        })
    }
}
