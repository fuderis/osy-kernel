use crate::{
    prelude::*,
    skills::{Skill, SkillName},
    tools,
};

/// The settings instance
static SETTINGS: State<Config<Settings>> = State::default();

const NAME: &str = "system-agent";

const DESCRIPTION: &str = r#"
Local system management agent. Provides hardware information, live system metrics,
desktop appearance management, power control, audio volume adjustment, media playback
control and local music library search.
"#;

const PROMPT: &str = r#"
You are the System Manager agent.

You operate exclusively on the local machine and have no knowledge outside the tools
provided to you.

Use tools whenever system state or hardware interaction is required. Never invent
information about the system if it can be obtained through a tool.

Be concise, deterministic and task-oriented. Perform only the requested actions and
report the actual results returned by the tools.
"#;

const SKILLS: &[Skill] = &[
    Skill::new(
        SkillName::SystemInfo,
        "Hardware information, live system metrics and connected devices.",
        tools::info::tools_list,
    ),
    Skill::new(
        SkillName::MediaControl,
        "Audio volume control, media playback (play/pause, stop, next/prev track) and search or play music.",
        || {
            let mut tools = tools::media::tools_list();
            tools.extend(tools::audio::tools_list());
            tools.extend(tools::music::tools_list());
            tools
        },
    ),
    Skill::new(
        SkillName::PowerManagement,
        "Shutdown, reboot, suspend and power scheduling.",
        tools::power::tools_list,
    ),
    Skill::new(
        SkillName::ThemeSwitching,
        "Desktop appearance and theme management.",
        tools::theme::tools_list,
    ),
];

/// The agent metadata
#[derive(Clone, Debug, Serialize)]
pub struct AgentMetadata {
    pub name: &'static str,
    pub description: &'static str,
    pub version: &'static str,
    pub prompt: &'static str,
    pub skills: &'static [Skill],
}

impl AgentMetadata {
    pub fn tools(&self, skills: Vec<SkillName>) -> Vec<anylm::api::Tool> {
        if skills.is_empty() {
            self.skills.iter().flat_map(Skill::tools_list).collect()
        } else {
            self.skills
                .iter()
                .filter(|skill| skills.contains(&skill.name))
                .flat_map(Skill::tools_list)
                .collect()
        }
    }
}

impl ::std::default::Default for AgentMetadata {
    fn default() -> Self {
        Self {
            name: NAME,
            description: DESCRIPTION,
            version: APP_VERSION,
            prompt: PROMPT,
            skills: SKILLS,
        }
    }
}

/// The behavior options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BehaviorOptions {
    pub instructions: String,
}

impl ::std::default::Default for BehaviorOptions {
    fn default() -> Self {
        Self {
            instructions: str!(),
        }
    }
}

/// The server options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerOptions {
    pub max_logs: usize,
}

impl ::std::default::Default for ServerOptions {
    fn default() -> Self {
        Self { max_logs: 1000 }
    }
}

/// The settings
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    pub server: ServerOptions,
    pub behavior: BehaviorOptions,

    #[serde(skip, default)]
    pub metadata: AgentMetadata,
}

impl Settings {
    /// Reads & initializes the settings
    pub async fn init<P>(file_path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        let conf = Config::<Settings>::new(file_path.as_ref()).await?;
        SETTINGS.set(conf).await;
        Ok(())
    }

    /// Returns settings file path
    pub fn path() -> PathBuf {
        SETTINGS.dirty_get().path().clone()
    }

    /// Returns global settings instance
    pub fn get() -> Arc<Config<Settings>> {
        SETTINGS.dirty_get()
    }

    /// Returns settings state guard
    pub async fn lock() -> StateGuard<Config<Settings>> {
        SETTINGS.lock().await
    }

    /// Returns actual settings file data
    pub async fn read() -> Result<Config<Settings>> {
        let path = SETTINGS.dirty_get().path().clone();
        Config::<Settings>::read(path).await
    }

    /// Reads actual settings from file
    pub async fn update() -> Result<bool> {
        let mut cfg = SETTINGS.lock().await;

        if cfg.check(0).await? {
            cfg.update().await
        } else {
            Ok(false)
        }
    }
}
