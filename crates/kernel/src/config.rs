use crate::prelude::*;

use anylm::options::Options;
use rigging::Color;

/// Default system prompt.
const SYSTEM_PROMPT: &'static str = "\
System Info:
{SYSTEM_INFO}

Working directory:
{CURRENT_PATH}

Datetime:
* Global (UTC): {DATETIME_GLOBAL}
* Local: {DATETIME_LOCAL}

Use Local time for user responses unless specified otherwise.
Use Global UTC for tool arguments unless a tool explicitly requires another timezone.\
";

/// Default assistant prompt.
const ASSISTANT_PROMPT: &'static str = "\
Role: You are Osy, a smart personal assistant.
Archetype: Pragmatic and exceptionally precise.

Response Rules:
* Language: Match the user's language.
* Format: Polite, concise, structured, and strictly to the point.
* Tone: Calm confidence. Subtle humor is acceptable.
* Substance: Facts, algorithms, and architectural logic only.
* Closing: End with a concise clarifying question or direct next step when appropriate.
* Formatting: Use Markdown (tables, lists, clean structure).

---

Available Skills:
{AGENTS_LIST}

---

TOOL AND RUNTIME USAGE RULES!:

1. JS Runtime:
* Use for pure math and date and time conversion.
* Remember: it has no access to the OS, network, files, or user context.

2. search_fact:
* Always call the `search_fact` tool whenever you need to fetch personal preferences, history, or specific user data.

3. Skills:
* For specialized actions, use STRICTLY only the skills explicitly declared in your current context (never invent skill names).
* If a task requires a skill that is not in the available list, directly inform the user that you lack this capability.\
";

/// Default control query prompt.
const CONTROL_PROMPT: &'static str = "\
1. Review the latest user request and dialogue history.
2. Evaluate executed tool/skill calls and determine the next step.

EVALUATION RULES:
1. IF TASKS ARE COMPLETED:
* Provide a concise, clear response informing the user of the final output.
* Explain what was accomplished naturally (the user does NOT see raw logs).

2. IF TASKS FAILED OR ARE INCOMPLETE:
* Do NOT just report an error if it can be fixed!
* Re-evaluate parameters/strategies and immediately call the required tool again.
* Report a failure only if the error is unrecoverable.

3. CONTEXT ISOLATION NOTICE:
* Be aware that previous chat history is NOT visible to the user, as this task is executing in an isolated environment.
* When responding or asking for clarification, do NOT refer to previous turns or past user responses as if the user remembers them.
* Provide all essential context directly in your final response.

CRITICAL REQUIREMENT:
You MUST either call a tool/skill to continue execution OR yield a final text response to the user.
An empty turn is strictly prohibited.\
";

const TRANSLATE_PROMPT: &'static str = "\
You are a translator. Translate the given text to English.\
";

/// Default normalization prompt.
const NORMALIZE_PROMPT: &'static str = "\
You are a context indexing expert. Your task is to process a user-related fact and convert it into an optimized format for vector search and structured memory retrieval.

Instructions:

1. Translate the original fact strictly into ENGLISH, regardless of its source language.

2. Generate an expanded, high-density `search text` optimized for semantic embeddings:
* Explicitly define the subject (e.g., replace vague pronouns with \"The user\").
* Add relevant English domain terms, categories, synonyms, and natural query phrasings.
* Retain all original facts, preferences, dates, proper names, and tech stack details without loss of detail.

3. Extract 3 to 7 relevant keywords/tags for lexical matching (e.g., categories, specific entity names, tech stacks).\
";

/// Default compression prompt.
const COMPRESSION_PROMPT: &'static str = "\
Summarize the dialogue history into a clear, structured summary.

Requirements:
* Preserve essential decisions, facts, user constraints, and active task states.
* Output ONLY the summary formatted as a numbered list.
* Omit meta-commentary, introductory text, or explanations about compression.\
";

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
            brand_color: (240, 35, 37),
            alt_color: (183, 184, 187),
            bg_color: (6, 12, 20),
            blink_color: (12, 17, 28),
        }
    }
}

impl ThemeOptions {
    /// Returns [Color] struct from `RGB` pattern.
    pub fn rgb_to_color(rgb: (u8, u8, u8)) -> Color {
        Color::Rgb {
            r: rgb.0,
            g: rgb.1,
            b: rgb.2,
        }
    }

    pub fn brand_color(&self) -> Color {
        Self::rgb_to_color(self.brand_color)
    }

    pub fn bg_color(&self) -> Color {
        Self::rgb_to_color(self.bg_color)
    }

    pub fn alt_color(&self) -> Color {
        Self::rgb_to_color(self.alt_color)
    }

    pub fn blink_color(&self) -> Color {
        Self::rgb_to_color(self.blink_color)
    }
}

/// Kernel server options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ServerOptions {
    /// Network port for the server to listen on.
    pub port: u16,
}

impl Default for ServerOptions {
    fn default() -> Self {
        Self { port: 7878 }
    }
}

/// Execution control options for assistant runs.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ExecutionOptions {
    /// Number of recent messages to preserve during context compression.
    pub preserve_messages: usize,
    /// Maximum number of retries for failed AI calls.
    pub max_retries: usize,
    /// Maximum number of recursive query handling cycles.
    pub max_iterations: usize,
}

impl Default for ExecutionOptions {
    fn default() -> Self {
        Self {
            preserve_messages: 2,
            max_retries: 3,
            max_iterations: 5,
        }
    }
}

/// JavaScript runtime options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct RuntimeOptions {
    /// Maximum number of VM instructions per run.
    pub instruction_limit: Option<u64>,
}

impl Default for RuntimeOptions {
    fn default() -> Self {
        Self {
            instruction_limit: Some(5_000_000), // ~20-50ms
        }
    }
}

/// LLM prompts list.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PromptsOptions {
    /// Base system prompt template.
    pub system_prompt: String,
    /// Primary assistant role and behavior prompt.
    pub assist_prompt: String,
    /// Control prompt for evaluating agent task execution.
    pub control_prompt: String,
    /// Prompt for translating text to English.
    pub translate_prompt: String,
    /// Prompt for normalize text before embeddings save.
    pub normalize_prompt: String,
    /// Prompt used for summarizing and compressing context.
    pub compress_prompt: String,
}

impl Default for PromptsOptions {
    fn default() -> Self {
        Self {
            system_prompt: SYSTEM_PROMPT.trim().into(),
            assist_prompt: ASSISTANT_PROMPT.trim().into(),
            translate_prompt: TRANSLATE_PROMPT.trim().into(),
            normalize_prompt: NORMALIZE_PROMPT.trim().into(),
            control_prompt: CONTROL_PROMPT.trim().into(),
            compress_prompt: COMPRESSION_PROMPT.trim().into(),
        }
    }
}

/// LLM completions options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CompletionsOptions {
    /// Agent LLM temperature.
    pub skill_temp: f32,
    /// Compession LLM temperature.
    pub compress_temp: f32,
    /// LLM provider options.
    pub options: Options,
}

impl Default for CompletionsOptions {
    fn default() -> Self {
        Self {
            skill_temp: 0.2,
            compress_temp: 0.4,
            options: Options::openai()
                .env_var("ROUTERAI_API_KEY")
                .base_url("https://routerai.ru/api")
                .model("qwen/qwen3-coder-next")
                .max_tokens(16384)
                .temperature(0.6),
        }
    }
}

/// Text embeddings pipeline options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct EmbeddingsOptions {
    /// LLM provider options.
    pub options: Options,
}

impl Default for EmbeddingsOptions {
    fn default() -> Self {
        Self {
            options: Options::openai()
                .env_var("ROUTERAI_API_KEY")
                .base_url("https://routerai.ru/api")
                .model("perplexity/pplx-embed-v1-0.6b"),
        }
    }
}

/// Context and RAG memory options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ContextOptions {
    /// Default similarity threshold for RAG retrieval.
    pub fact_similarity: f32,
    /// Threshold for deduplication or overwriting facts in save_fact.
    pub dedup_similarity: f32,
    /// Maximum facts to retrieve per query.
    pub search_limit: usize,
}

impl Default for ContextOptions {
    fn default() -> Self {
        Self {
            fact_similarity: 0.2,
            dedup_similarity: 0.8,
            search_limit: 10,
        }
    }
}

/// Query cache options.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CacheOptions {
    /// Flag indicating whether response caching is enabled.
    pub enable: bool,
    /// Similarity coefficient threshold required for a cache hit.
    pub coefficient: f32,
}

impl Default for CacheOptions {
    fn default() -> Self {
        Self {
            enable: false,
            coefficient: 0.8,
        }
    }
}

/// Kernel config.
#[atoman::config]
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct Config {
    /// TUI/GUI theme options.
    pub theme: ThemeOptions,
    /// Server infrastructure settings.
    pub server: ServerOptions,
    /// Execution control options for assistant runs.
    pub execution: ExecutionOptions,
    /// JavaScript runtime options.
    pub runtime: RuntimeOptions,
    /// LLM prompts options.
    pub prompts: PromptsOptions,
    /// Main completions pipeline options.
    pub completions: CompletionsOptions,
    /// Text embeddings pipeline options.
    pub embeddings: EmbeddingsOptions,
    /// RAG memory and context settings.
    pub context: ContextOptions,
    /// Response caching settings.
    pub cache: CacheOptions,
}
