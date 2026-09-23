use crate::prelude::*;

use atoman::{process::Command, time::sleep};
use osy_share::{AgentMeta, StatusData};
use rigging::{Stylize, widgets::Print};
use std::{net::TcpListener, time::Duration};

/// API: Handles server status checking.
pub async fn handle_server_status() -> Result<()> {
    let port = str!(Config::get().server.port);
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
pub async fn handle_server_start() -> Result<()> {
    Print::h1("Starting Server:").render().await?;

    let cfg = Config::get();

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

    Print::success("Ready for requests!")
        .margin_top(1)
        .render()
        .await?;
    println!();

    Ok(())
}

/// API: Handles server shutdown.
pub async fn handle_server_stop() -> Result<()> {
    Print::h1("Stopping Server:").render().await?;

    let cfg = Config::get();
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

    Print::success("Processes terminated.")
        .margin_top(1)
        .render()
        .await?;
    println!();

    Ok(())
}

/// API: Handles server restarting.
pub async fn handle_server_restart() -> Result<()> {
    handle_server_stop().await?;
    sleep(Duration::from_millis(500)).await;
    handle_server_start().await?;

    Ok(())
}
