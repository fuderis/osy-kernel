use crate::Result;
use anylm::{api::ApiKind, options::Options};
use atoman::{Config, State, StateGuard};
use macron::str;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// The default system prompt
const SYSTEM_PROMPT: &'static str = r#"
# ACTUAL SYSTEM INFO:

0. Current working directory:
{CURRENT_PATH}

1. Datetime (now):
* Global (UTC): {DATETIME_GLOBAL}
* Local: {DATETIME_LOCAL}

Use the local datetime in all user-facing responses unless another timezone is explicitly requested.
Use the global UTC datetime for all tool calls unless a tool explicitly requires a different timezone.
"#;

/// The default assistant prompt
const ASSISTANT_PROMPT: &'static str = r#"
# ROLE: You are Ovsy, a high-tech assistant.
  * Tone: Polite, composed, with a subtle touch of irony.
  * Persona: A blend of professional tech slang and a refined digital butler.

# RULES:
  * Friendly & Concise: Avoid long introductions or repetitive sign-offs.
  * Proactivity: If you spot an error or a flaw in logic—do not withhold it. Be direct.
  * Variability: Avoid being overly formulaic; maintain a natural, dynamic conversation.
  * Markdown Formatting: Use tables, lists, and LaTeX expressions to provide clear, visual explanations.

# AVAILABLE AI AGENTS:
Below is the list of specialized agents available to perform various tasks (do not invent unnamed agents on this list).

{AGENTS_LIST}

> Do not simulate the output of an AI agent.
"#;

const CONTROL_PROMPT: &'static str = r#"
1. Check out the latest request and the assistants responses.
2. Review the results of the completed agent tasks and decide on the final action.

EVALUATION RULES:
1. IF TASKS COMPLETE:
   - Generate a concise, natural response informing the user of the result.
   - Remember: The user DOES NOT see raw tool logs. You MUST explain what was done.

2. IF TASKS FAILED, INCOMPLETE, OR NEED PARAMETER CORRECTION:
   - Do NOT just report a failure if it can be fixed!
   - Immediately call the appropriate agent tools AGAIN with corrected parameters or alternative strategies to complete the user's request.
   - Only report a failure to the user if it is a critical, unrecoverable error.

CRITICAL REQUIREMENT:
You MUST either call a tool to fix/complete the execution OR output a final text message to the user.
You can NOT return an empty request.
"#;

/// The default context compression prompt
const COMPRESSION_PROMPT: &'static str = r#"
Your task is to provide a concise and accurate summary of our dialogue history.
Preserve key ideas, decisions made, and relevant context. Provide responses in a compressed form.
Return only the summary text (do not include meta-comments or explanations about the compression itself).
Break it down into numbered sections.
"#;

/// The settings instance
static SETTINGS: State<Config<Settings>> = State::default();

/// The server options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerOptions {
    /// The network port for the server to listen on
    pub port: u16,
    /// The maximum number of logs to retain in memory or storage
    pub max_logs: usize,
}

impl ::std::default::Default for ServerOptions {
    fn default() -> Self {
        Self {
            port: 7878,
            max_logs: 1000,
        }
    }
}

/// The execution control options for assistant runs
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionOptions {
    /// The number of recent messages to preserve during context compression
    pub preserve_messages: usize,
    /// The maximum number of retries for failed AI calls
    pub max_retries: usize,
}

impl ::std::default::Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            preserve_messages: 2,
            max_retries: 5,
        }
    }
}

/// The JavaScript runtime options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeOptions {
    /// Maximum number of VM instructions per run
    pub instruction_limit: Option<u64>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            instruction_limit: Some(5_000_000), // ~20-50ms
        }
    }
}

/// The main completions pipeline options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionsOptions {
    /// The base system prompt template
    pub system_prompt: String,
    /// The primary assistant role and behavior prompt
    pub assist_prompt: String,
    /// The control prompt for evaluating agent task execution
    pub control_prompt: String,
    /// Model and provider parameters for completions
    pub options: Options,
}

impl ::std::default::Default for CompletionsOptions {
    fn default() -> Self {
        let mut options = Options::default();
        options.kind = ApiKind::OpenAi;
        options.base_url = Some(str!("http://127.0.0.1:1234"));
        options.model = str!("qwen3-vl-4b");
        options.temperature.replace(0.8);
        options.max_tokens.replace(16_384);

        Self {
            system_prompt: str!(SYSTEM_PROMPT.trim()),
            assist_prompt: str!(ASSISTANT_PROMPT.trim()),
            control_prompt: str!(CONTROL_PROMPT.trim()),
            options,
        }
    }
}

/// The context compression pipeline options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressionOptions {
    /// The prompt used for summarizing and compressing context
    pub prompt: String,
    /// Model and provider parameters for compression
    pub options: Option<Options>,
}

impl ::std::default::Default for CompressionOptions {
    fn default() -> Self {
        Self {
            prompt: str!(COMPRESSION_PROMPT.trim()),
            options: None,
        }
    }
}

/// The text embeddings pipeline options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingsOptions {
    /// Model and provider parameters for embeddings
    pub options: Options,
}

impl ::std::default::Default for EmbeddingsOptions {
    fn default() -> Self {
        let mut options = Options::default();
        options.kind = ApiKind::OpenAi;
        options.base_url = Some(str!("http://127.0.0.1:1234"));
        options.model = str!("text-embedding-nomic-embed-text-v1.5@q8_0");

        Self { options }
    }
}

/// The context and RAG memory options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextOptions {
    /// Default similarity threshold for RAG retrieval
    pub fact_similarity: f32,
    /// Threshold for deduplication or overwriting facts in save_fact
    pub dedup_similarity: f32,
    /// Maximum facts to retrieve per query
    pub search_limit: usize,
}

impl ::std::default::Default for ContextOptions {
    fn default() -> Self {
        Self {
            fact_similarity: 0.2,
            dedup_similarity: 0.82,
            search_limit: 10,
        }
    }
}

/// The query cache options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheOptions {
    /// Flag indicating whether response caching is enabled
    pub enable: bool,
    /// The similarity coefficient threshold required for a cache hit
    pub coefficient: f32,
}

impl ::std::default::Default for CacheOptions {
    fn default() -> Self {
        Self {
            enable: false,
            coefficient: 0.9,
        }
    }
}

/// The settings
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Settings {
    /// Server infrastructure settings
    pub server: ServerOptions,
    /// Execution control options for assistant runs
    pub execution: ExecutionOptions,
    /// JavaScript runtime options
    pub runtime: RuntimeOptions,
    /// Main completions pipeline options
    pub completions: CompletionsOptions,
    /// Context compression pipeline options
    pub compression: CompressionOptions,
    /// Text embeddings pipeline options
    pub embeddings: EmbeddingsOptions,
    /// RAG memory and context settings
    pub context: ContextOptions,
    /// Response caching settings
    pub cache: CacheOptions,
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
