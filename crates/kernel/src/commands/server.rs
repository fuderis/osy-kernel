use crate::prelude::*;

use osy_share::{AgentMeta, StatusData};
use rigging::{Stylize, widgets::Print};
use std::{net::TcpListener, process::Stdio, time::Duration};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

const TIMEOUT: Duration = Duration::from_millis(500);

/// API: Handles server status checking.
pub async fn handle_status() -> Result<()> {
    let port = str!(Settings::get().server.port);
    let client = Client::tcp();

    Print::h1("Kernel Server:").render().await?;

    // checking server
    match client
        .get(&format!("http://127.0.0.1:{port}/status"))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                Print::new()
                    .field("Status", str!("Online".green()))
                    .field("Port", str!(port.green()))
                    .render()
                    .await?;

                let data: StatusData = response
                    .json()
                    .await
                    .map_err(|e| Error::Custom(format!("Failed to parse response: {e}")))?;

                let mut agents = Print::field("Agents", "");

                if !data.agents_list.is_empty() {
                    for AgentMeta {
                        name, description, ..
                    } in data.agents_list
                    {
                        agents =
                            agents.tree_item(format!("{} — {}", name.bold(), description.trim()));
                    }

                    agents.render().await?;
                } else {
                    agents.tree_item("No agents loaded.").render().await?;
                }
            } else {
                let err_msg = response
                    .text()
                    .await
                    .unwrap_or_else(|_| str!("Failed to read error body"));

                Print::error(format!("Server error ({status}): {err_msg}"))
                    .margin_top(1)
                    .render()
                    .await?;
            }
        }
        Err(_) => {
            Print::field("Status", str!("Offline".red()))
                .render()
                .await?;
        }
    }

    println!();
    Ok(())
}

/// API: Handles server launching.
pub async fn handle_start(start_lms: bool) -> Result<()> {
    Print::h1("Starting Server:").render().await?;

    let cfg = Settings::get();

    // start server
    let port = str!(cfg.server.port);
    let is_port_free = TcpListener::bind(format!("127.0.0.1:{port}")).is_ok();

    if is_port_free {
        Command::new(path!("$"))
            .arg("serve")
            .current_dir(path!("$/"))
            .kill_on_drop(false)
            .spawn()?;

        Print::new()
            .field("Status", str!("Online".green()))
            .field("Port", str!(port.green()))
            .render()
            .await?;
    } else {
        Print::warn(format!("Port {port} is already in use..."))
            .render()
            .await?;
    }

    // start LMS server
    if start_lms {
        Print::h1("Starting LMS Server:")
            .margin_top(1)
            .render()
            .await?;

        let is_running = match timeout(TIMEOUT, Command::new("lms").args(["status"]).output()).await
        {
            Ok(Ok(out)) => String::from_utf8_lossy(&out.stdout).contains("ON"),
            _ => false,
        };

        if !is_running {
            match Command::new("lms")
                .args(["server", "start"])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
            {
                Ok(_child) => {
                    let mut is_ok = false;

                    // 100 tries * 100 ms = 10 seconds to start
                    for _ in 0..100 {
                        sleep(Duration::from_millis(100)).await;

                        let status_check =
                            timeout(TIMEOUT, Command::new("lms").args(["status"]).output()).await;

                        if let Ok(Ok(out)) = status_check {
                            if String::from_utf8_lossy(&out.stdout).contains("ON") {
                                is_ok = true;
                                break;
                            }
                        }
                    }

                    if is_ok {
                        Print::field("Status", str!("Online".green()))
                            .render()
                            .await?;
                    } else {
                        Print::warn(str!("LMS server failed to start...".red()))
                            .render()
                            .await?;
                    }
                }

                Err(e) => {
                    return Err(
                        Error::Titled("Failed to spawn LMS process".into(), e.into()).into(),
                    );
                }
            }
        } else {
            Print::field("Status", str!("Online".green()))
                .render()
                .await?;
        }
    }

    Print::success("Ready for requests!")
        .margin_top(1)
        .render()
        .await?;
    println!();

    Ok(())
}

/// API: Handles server shutdown.
pub async fn handle_stop(stop_lms: bool) -> Result<()> {
    Print::h1("Stopping Server:").render().await?;

    let cfg = Settings::get();
    let port = cfg.server.port;

    // stop server
    #[cfg(unix)]
    {
        let _ = Command::new("sh")
            .args(["-c", &format!("fuser -k {}/tcp", port)])
            .output()
            .await;
    }
    #[cfg(windows)]
    {
        let cmd = format!(
            "for /f \"tokens=5\" %a in ('netstat -aon ^| findstr \":{}\"') do taskkill /f /pid %a",
            port
        );
        let _ = Command::new("cmd").args(["/C", &cmd]).output().await;
    }

    Print::field("Status", str!("Offline".red()))
        .render()
        .await?;

    // stop LMS server
    if stop_lms {
        Print::h1("Stopping LMS Server:")
            .margin_top(1)
            .render()
            .await?;

        // unload models first
        let _ = timeout(
            TIMEOUT,
            Command::new("lms").args(["unload", "--all"]).output(),
        )
        .await;

        let fields = Print::field("Models", str!("Unloaded".red()));

        let _ = timeout(
            TIMEOUT,
            Command::new("lms")
                .args(["server", "stop"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status(),
        )
        .await;

        fields
            .field("Status", str!("Offline".red()))
            .render()
            .await?;
    }

    Print::success("Processes terminated.")
        .margin_top(1)
        .render()
        .await?;
    println!();

    Ok(())
}

/// API: Handles server restarting.
pub async fn handle_restart(restart_lms: bool) -> Result<()> {
    handle_stop(restart_lms).await?;
    sleep(Duration::from_millis(500)).await;
    handle_start(restart_lms).await?;

    Ok(())
}
