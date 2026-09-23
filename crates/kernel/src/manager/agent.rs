use super::{MANAGER, Manager};
use crate::prelude::*;

use atoman::{
    net::UnixStream,
    process::{Child, Command},
    sync::Mutex,
    time,
};
use osy_share::AgentMeta;
use pearce::Client;
use std::{
    process::Stdio,
    sync::Arc,
    time::{Duration, SystemTime},
};

/// Agent instance.
#[derive(Default, Debug, Clone)]
pub struct Agent {
    /// Execution file path.
    pub exec_path: PathBuf,
    /// Agent metadata.
    pub metadata: AgentMeta,

    /// Agent server start time.
    _started: Option<SystemTime>,
    /// Agent server process child.
    _child: Arc<Mutex<Option<Child>>>,
}

impl Agent {
    /// Runs agent server.
    #[log()]
    pub async fn run(exec_path: impl Into<PathBuf>) -> Result<()> {
        let exec_path = exec_path.into();
        info!("[Manager] Starting agent `{}`...", exec_path.display());

        // extract file name
        let file_name = exec_path
            .file_stem()
            .ok_or(Error::FetchAgentFileName)?
            .to_string_lossy()
            .to_string();

        // fetch metadata before running the server
        let meta_output = Command::new(&exec_path).arg("metadata").output().await?;
        if !meta_output.status.success() {
            let stderr = String::from_utf8_lossy(&meta_output.stderr);
            return Err(Error::FetchAgentMetadata {
                name: file_name,
                source: stderr.into(),
            }
            .into());
        }

        let metadata: AgentMeta = serde_json::from_slice(&meta_output.stdout)?;

        // check agent for already running
        if Manager::has_agent(&arc!(metadata.name.clone())).await {
            info!(
                "[Manager] Agent `{}` already in running, skipping...",
                metadata.name
            );
            return Ok(());
        }

        // build server execution command
        let mut cmd = Command::new(&exec_path);
        cmd.arg("serve");
        cmd.stdin(Stdio::piped()).kill_on_drop(true);

        // process binding to the kernel server
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
        #[cfg(windows)]
        let child = cmd.spawn_group()?;
        #[cfg(not(windows))]
        let child = cmd.spawn()?;

        // ping server until it wakes up (50 tries = 5 sec)
        let client = Client::ipc(&metadata.sock_path.to_string_lossy());
        for attempt in 1..=50 {
            let request_result =
                time::timeout(Duration::from_millis(100), client.get("/ping").send()).await;

            match request_result {
                Ok(Ok(response)) if response.status().is_success() => break,
                _ if attempt >= 50 => {
                    return Err(Error::AgentNotLaunched {
                        name: metadata.name,
                        sock_path: str!(metadata.sock_path.to_string_lossy()),
                    }
                    .into());
                }
                _ => {
                    time::sleep(Duration::from_millis(100)).await;
                    continue;
                }
            }
        }

        let agent = Self {
            exec_path,
            metadata,
            _started: Some(SystemTime::now()),
            _child: Arc::new(Mutex::new(Some(child))),
        };

        // register in manager
        let name = agent.metadata.name.clone();
        MANAGER.get().agents.insert(name.clone(), agent).await;

        info!("[Manager] Agent `{name}` added to manager.");
        Manager::gen_doc().await;

        Ok(())
    }

    /// Stops agent server.
    #[log()]
    pub async fn stop(&self) -> Result<()> {
        let name = &self.metadata.name;
        info!("[Manager] Trying to stop `{name}` agent...");

        if MANAGER.get().agents.remove(name).await.is_some() {
            info!("[Manager] Agent `{name}` stopped.");
            Manager::gen_doc().await;
        } else {
            warn!("[Manager] Agent `{name}` is already stopped.");
        }

        Ok(())
    }

    /// Returns true if agent is outdated.
    #[log()]
    pub async fn check(&self, deep_check: bool) -> Result<bool> {
        // check socket connection
        let is_alive = time::timeout(
            Duration::from_millis(500),
            UnixStream::connect(&self.metadata.sock_path),
        )
        .await;

        if is_alive.is_err() || is_alive.unwrap().is_err() {
            let name = &self.metadata.name;
            info!("[Manager] Agent `{name}` not responding...");
            return Ok(true);
        }

        // check execution file metadata
        if deep_check {
            let metadata = atoman::fs::metadata(&self.exec_path).await?;

            if let Ok(modified_at) = metadata.modified()
                && let Some(started_at) = self._started
            {
                return Ok(modified_at > started_at);
            }
        };

        Ok(false)
    }

    /// Ensures the agent is running
    /// (returns true if ready for queries)
    #[log()]
    pub async fn ensure(&self) -> Result<()> {
        let name = &self.metadata.name;
        let exec_path = &self.exec_path;

        // check agent health
        if self.check(false).await.unwrap_or(true) {
            if !exec_path.exists() {
                error!(
                    "[Manager] Agent `{name}` binary not found at `{}`.",
                    exec_path.display()
                );

                let name = name.clone();
                let sock_path = self.metadata.sock_path.to_string_lossy().into();
                return Err(Error::AgentNotLaunched { name, sock_path }.into());
            }

            // stop agent
            warn!("[Manager] Agent `{name}` is outdated.");
            let _ = self.stop().await;

            // run agent again
            if let Err(e) = Self::run(exec_path).await {
                error!("[Manager] Agent `{name}` failed to restart: {e}");
                Manager::gen_doc().await;
                return Err(e);
            } else {
                Manager::gen_doc().await;
            }
        }

        Ok(())
    }
}
