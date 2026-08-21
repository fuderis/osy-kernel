use crate::Result;
use anylm::{api::ApiKind, options::Options};
use atoman::{Config, State, StateGuard};
use macron::str;
use rigging::Color;
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    sync::Arc,
};

/// The default system prompt
const SYSTEM_PROMPT: &'static str = r#"
Working directory:
{CURRENT_PATH}

Datetime:
- Global (UTC): {DATETIME_GLOBAL}
- Local: {DATETIME_LOCAL}

Use Local time for user responses unless specified otherwise.
Use Global UTC for tool arguments unless a tool explicitly requires another timezone.
"#;

/// The default assistant prompt
const ASSISTANT_PROMPT: &'static str = r#"
Working directory:
{CURRENT_PATH}

Datetime:
- Global (UTC): {DATETIME_GLOBAL}
- Local: {DATETIME_LOCAL}

Use Local time for user responses unless specified otherwise.
Use Global UTC for tool arguments unless a tool explicitly requires another timezone.
"""

assist_prompt = """
Role: You are Osy, a smart personal assistant.
Archetype: Pragmatic and exceptionally precise.

Response Rules:
- Language: Match the user's language.
- Format: Polite, concise, structured, and strictly to the point.
- Tone: Calm confidence. Subtle humor is acceptable.
- Substance: Facts, algorithms, and architectural logic only.
- Closing: End with a concise clarifying question or direct next step when appropriate.
- Formatting: Use Markdown (tables, lists, clean structure).

---

Available Skills:
{AGENTS_LIST}

---

TOOL AND RUNTIME USAGE RULES!:

1. JS Runtime:
  - Use for pure math and date and time conversion.
  - Remember: it has no access to the OS, network, files, or user context.

2. search_fact:
  - Always call the `search_fact` tool whenever you need to fetch personal preferences, history, or specific user data.

3. Skills:
  - For specialized actions, use STRICTLY only the skills explicitly declared in your current context (never invent skill names).
  - If a task requires a skill that is not in the available list, directly inform the user that you lack this capability.
"#;

/// The default contorl prompt
const CONTROL_PROMPT: &'static str = r#"
1. Review the latest user request and dialogue history.
2. Evaluate executed tool/skill calls and determine the next step.

EVALUATION RULES:
1. IF TASKS ARE COMPLETED:
   - Provide a concise, clear response informing the user of the final output.
   - Explain what was accomplished naturally (the user does NOT see raw logs).

2. IF TASKS FAILED OR ARE INCOMPLETE:
   - Do NOT just report an error if it can be fixed!
   - Re-evaluate parameters/strategies and immediately call the required tool again.
   - Report a failure only if the error is unrecoverable.

CRITICAL REQUIREMENT:
You MUST either call a tool/skill to continue execution OR yield a final text response to the user.
An empty turn is strictly prohibited.
"#;

/// The default normalization prompt
const NORMALIZE_PROMPT: &'static str = r#"
You are a context indexing expert. Your task is to process a user-related fact and convert it into an optimized format for vector search and structured memory retrieval.

Instructions:
1. Translate the original fact strictly into ENGLISH, regardless of its source language.
2. Generate an expanded, high-density `search text` optimized for semantic embeddings:
   - Explicitly define the subject (e.g., replace vague pronouns with "The user").
   - Add relevant English domain terms, categories, synonyms, and natural query phrasings.
   - Retain all original facts, preferences, dates, proper names, and tech stack details without loss of detail.
3. Extract 3 to 7 relevant keywords/tags for lexical matching (e.g., categories, specific entity names, tech stacks).
"#;

/// The default compression prompt
const COMPRESSION_PROMPT: &'static str = r#"
Summarize the dialogue history into a clear, structured summary.

Requirements:
- Preserve essential decisions, facts, user constraints, and active task states.
- Output ONLY the summary formatted as a numbered list.
- Omit meta-commentary, introductory text, or explanations about compression.
"#;

/// The settings instance
static SETTINGS: State<Config<Settings>> = State::default();

/// Theme color palette settings (for rigging widgets).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ThemeOptions {
    pub brand_color: (u8, u8, u8),
    pub alt_color: (u8, u8, u8),
    pub bg_color: (u8, u8, u8),
    pub blink_color: (u8, u8, u8),
}

impl Default for ThemeOptions {
    fn default() -> Self {
        Self {
            brand_color: (255, 85, 65),
            alt_color: (140, 120, 81),
            bg_color: (13, 17, 29),
            blink_color: (20, 26, 42),
        }
    }
}

impl ThemeOptions {
    pub fn brand_color(&self) -> Color {
        Color::Rgb {
            r: self.brand_color.0,
            g: self.brand_color.1,
            b: self.brand_color.2,
        }
    }

    pub fn bg_color(&self) -> Color {
        Color::Rgb {
            r: self.bg_color.0,
            g: self.bg_color.1,
            b: self.bg_color.2,
        }
    }

    pub fn alt_color(&self) -> Color {
        Color::Rgb {
            r: self.alt_color.0,
            g: self.alt_color.1,
            b: self.alt_color.2,
        }
    }

    pub fn blink_color(&self) -> Color {
        Color::Rgb {
            r: self.blink_color.0,
            g: self.blink_color.1,
            b: self.blink_color.2,
        }
    }
}

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
    /// The embeddings normalization prompt
    pub normalize_prompt: String,
    /// The prompt used for summarizing and compressing context
    pub compression_prompt: String,

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
            normalize_prompt: str!(NORMALIZE_PROMPT),
            control_prompt: str!(CONTROL_PROMPT.trim()),
            compression_prompt: str!(COMPRESSION_PROMPT.trim()),
            options,
        }
    }
}

/// The context compression pipeline options
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompressionOptions {
    /// Model and provider parameters for compression
    pub options: Option<Options>,
}

impl ::std::default::Default for CompressionOptions {
    fn default() -> Self {
        Self { options: None }
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
    pub theme: ThemeOptions,
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
