use super::Manager;
use crate::prelude::*;

use osy_share::AgentMetadata;
use pearce::Client;
use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime},
};
use tokio::{
    net::UnixStream,
    process::{Child, Command},
    sync::Mutex,
    time,
};

/// The AI agent
#[derive(Default, Debug, Clone)]
pub struct Agent {
    pub exec_path: PathBuf,
    pub sock_path: PathBuf,
    pub metadata: AgentMetadata,
    _started: Option<SystemTime>,
    _child: Arc<Mutex<Option<Child>>>,
}

impl Agent {
    /// Runs the agent server
    pub async fn run(exec_path: impl Into<PathBuf>) -> Result<Option<Self>> {
        let exec_path = exec_path.into();

        // extract file name
        let file_name = exec_path
            .file_stem()
            .ok_or(Error::FailedGetAgentName)?
            .to_string_lossy()
            .to_string();

        // fetch metadata before running the server
        let meta_output = Command::new(&exec_path).arg("metadata").output().await?;
        if !meta_output.status.success() {
            let stderr = String::from_utf8_lossy(&meta_output.stderr);
            return Err(Error::FailedFetchMetadata {
                name: file_name,
                source: stderr.into(),
            }
            .into());
        }

        let metadata: AgentMetadata = serde_json::from_slice(&meta_output.stdout)?;

        // check agent for already running:
        if Manager::contains(&arc!(metadata.name.clone())).await {
            info!(
                "[Manager] Agent '{}' already in running, skipping...",
                metadata.name
            );
            return Ok(None);
        }

        // setup Unix Domain Socket path
        let sock_path = path!("$temp$/socks/{}.sock", metadata.name);

        // build server execution command
        let mut cmd = Command::new(&exec_path);
        cmd.arg("serve");
        cmd.stdin(Stdio::piped()).kill_on_drop(true);

        #[cfg(target_os = "linux")]
        {
            unsafe {
                cmd.pre_exec(|| {
                    if libc::prctl(libc::PR_SET_PDEATHSIG, libc::SIGKILL) == -1 {
                        return Err(std::io::Error::last_os_error());
                    }
                    Ok(())
                });
            }
        }

        let child = cmd.spawn()?;

        // 4. Ping the server via POST /ping until it wakes up
        let client = Client::ipc(&sock_path.to_string_lossy());
        let mut attempts = 0;

        loop {
            attempts += 1;

            let request_result =
                time::timeout(Duration::from_millis(100), client.get("/ping").send()).await;

            match request_result {
                Ok(Ok(response)) if response.status().is_success() => {
                    // check if response body is "pong" or returns successfully
                    break;
                }
                _ => {
                    if attempts >= 50 {
                        return Err(Error::AgentStartFailed {
                            name: metadata.name,
                            sock_path: sock_path.to_string_lossy().to_string(),
                        }
                        .into());
                    }

                    tokio::time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
        }

        let agent = Self {
            exec_path,
            sock_path,
            metadata,
            _started: Some(SystemTime::now()),
            _child: arc_mutex!(Some(child)),
        };

        Ok(Some(agent))
    }

    /// Returns true if needs to be updated
    pub async fn check(&self) -> Result<bool> {
        let is_alive = time::timeout(
            Duration::from_millis(100),
            UnixStream::connect(&self.sock_path),
        )
        .await;

        if is_alive.is_err() || is_alive.unwrap().is_err() {
            // agent not responding..
            return Ok(true);
        }

        // check file metadata:
        let metadata = tokio::fs::metadata(&self.exec_path).await?;

        if let Ok(modified_at) = metadata.modified()
            && let Some(started_at) = self._started
        {
            Ok(modified_at > started_at)
        } else {
            Ok(false)
        }
    }
}
