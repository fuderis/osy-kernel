use crate::prelude::*;

use atoman::trace::{FilterEngine, MultiTrace, SourceConfig};
use regex::Regex;
use rigging::widgets::Print;

/// Server status.
#[derive(Deserialize)]
struct StatusResponse {
    /// List of active agents.
    pub agents_list: Vec<AgentMeta>,
}

/// Agent metadata (minimal).
#[derive(Deserialize)]
struct AgentMeta {
    /// Agent name.
    pub name: String,
}

/// Streams server logs.
pub async fn handle_trace(uid_filter: Option<u64>, only_new: bool) -> Result<()> {
    let settings = Config::get();
    let port = settings.server.port;
    let base_url = format!("http://127.0.0.1:{port}");

    // query to `/status` endpoint
    let agents: Vec<AgentMeta> = match Client::tcp()
        .get(&format!("{base_url}/status"))
        .timeout(Duration::from_secs(2))
        .send()
        .await
    {
        Ok(res) if res.status().is_success() => res
            .json::<StatusResponse>()
            .await
            .map(|r| r.agents_list)
            .unwrap_or_default(),
        _ => {
            Print::warn(format!("Server is unreachable on port {port}."))
                .render()
                .await?;

            vec![]
        }
    };

    // build list of log sources
    let mut sources = Vec::new();

    // primary kernel source
    let kernel_name = path!("$")
        .file_name()
        .map(|s| str!(s.to_string_lossy()))
        .unwrap_or("osy".into());
    sources.push(SourceConfig {
        name: kernel_name.clone(),
        dir_path: path!("$state/{kernel_name}/logs"),
        entry_start_pattern: Regex::new(r"\b\d{4}-\d{2}-\d{2}").unwrap(),
        color_code: 35,
    });

    // dynamic agent sources
    for agent in agents {
        let agent_name = format!("{kernel_name}-{}", agent.name);
        let agent_dir = path!("$state/{agent_name}/logs");
        sources.push(SourceConfig {
            name: agent_name,
            dir_path: agent_dir,
            entry_start_pattern: Regex::new(r"\b\d{4}-\d{2}-\d{2}").unwrap(),
            color_code: 36,
        });
    }

    // configure log entry filtering by UID or SID
    let mut regex_patterns = Vec::new();

    if let Some(uid) = uid_filter {
        let uid_sid_pattern = format!(r"(uid[=:\s]+{0}\b|(?:\b|/){0}-\d+-\d+)", uid);
        regex_patterns.push(uid_sid_pattern);

        Print::info(format!("Filtering logs by User ID: {uid}"))
            .render()
            .await?;
    }

    let raw_regex_refs: Vec<&str> = regex_patterns.iter().map(|s| s.as_str()).collect();
    let filter_engine = FilterEngine::new(&raw_regex_refs, &[]);

    // start multi-source log tracing
    let mut tracer =
        MultiTrace::start(sources, Duration::from_millis(200), filter_engine, only_new).await;

    Print::info("Log tracer started. Waiting for events...")
        .render()
        .await?;

    loop {
        tracer.recv_and_print().await;
    }
}
