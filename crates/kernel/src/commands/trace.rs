use crate::prelude::*;

use atoman::trace::{FilterEngine, MultiTrace, SourceConfig};
use regex::Regex;
use std::time::Duration;

/// Deserializes the JSON response returned by the `/status` endpoint.
#[derive(Deserialize)]
struct StatusResponse {
    /// A collection of active agent metadata objects.
    pub agents_list: Vec<AgentMetadata>,
}

/// Metadata information representing an individual active agent.
#[derive(Deserialize)]
struct AgentMetadata {
    /// The unique identifier or name of the agent.
    pub name: String,
}

/// Asynchronously streams and traces system and agent log files based on dynamic criteria.
///
/// This function queries the local core server to discover active agents, constructs log source
/// configurations for both the primary core kernel and any discovered agents, configures optional
/// regex filtering for user IDs (UIDs) or session IDs (SIDs), and executes a continuous streaming loop.
///
/// # Arguments
///
/// * `uid_filter` - An optional 64-bit unsigned integer representing the UID/SID to filter log records.
/// * `only_new` - If set to `true`, ignores existing historical logs and streams only newly appended events.
///
/// # Errors
///
/// Returns an [`Error::Custom`] if the underlying HTTP client cannot be initialized properly.
/// Note that if the `/status` request fails (e.g., if the core server is offline), the error is handled
/// internally by logging a warning and falling back to tracing core kernel logs only.
pub async fn handle_trace(uid_filter: Option<u64>, only_new: bool) -> Result<()> {
    let settings = Settings::get();
    let port = settings.server.port;
    let base_url = format!("http://127.0.0.1:{}", port);

    // 1. Attempt to query the /status endpoint to discover active agents.
    let client = reqwest::Client::builder()
        .timeout(Duration::from_secs(2))
        .build()
        .map_err(|e| Error::Custom(format!("Failed to build HTTP client: {}", e)))?;

    let agents: Vec<AgentMetadata> = match client.get(format!("{}/status", base_url)).send().await {
        Ok(res) if res.status().is_success() => res
            .json::<StatusResponse>()
            .await
            .map(|r| r.agents_list)
            .unwrap_or_default(),
        _ => {
            println!(
                "[WARN] Core server is unreachable on port {}. Tracing OSY-CORE logs only.",
                port
            );
            vec![]
        }
    };

    // 2. Build the list of log source configurations.
    let mut sources = Vec::new();

    // Primary OSY-CORE kernel source configuration.
    sources.push(SourceConfig {
        name: "OSY-KERNEL".into(),
        dir_path: path!("$state/osy/logs"),
        entry_start_pattern: Regex::new(r"\b\d{4}-\d{2}-\d{2}").unwrap(),
        color_code: 35,
    });

    // Dynamic OSY-AGENTNAME source configurations for discovered agents.
    for agent in agents {
        let agent_dir = path!("$state/osy-{}/logs", agent.name);
        sources.push(SourceConfig {
            name: str!("OSY-{}", agent.name.to_uppercase()),
            dir_path: agent_dir,
            entry_start_pattern: Regex::new(r"\b\d{4}-\d{2}-\d{2}").unwrap(),
            color_code: 36,
        });
    }

    // 3. Configure log entry filtering by UID or SID (e.g., SID format `1-8908959835-27588`).
    let mut regex_patterns = Vec::new();

    if let Some(uid) = uid_filter {
        let uid_sid_pattern = format!(r"(uid[=:\s]+{0}\b|(?:\b|/){0}-\d+-\d+)", uid);
        regex_patterns.push(uid_sid_pattern);
        println!("[INFO] Filtering logs by UID/SID: {}", uid);
    }

    let raw_regex_refs: Vec<&str> = regex_patterns.iter().map(|s| s.as_str()).collect();
    let filter_engine = FilterEngine::new(&raw_regex_refs, &[]);

    // 4. Initialize and launch multi-source log reading threads.
    let mut tracer =
        MultiTrace::start(sources, Duration::from_millis(200), filter_engine, only_new).await;

    println!("[INFO] Log tracer started. Waiting for events...\n");

    loop {
        tracer.recv_and_print().await;
    }
}
