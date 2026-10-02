use crate::prelude::*;

use atoman::{process::Command, time::sleep};
use osy_share::{AgentMeta, StatusData};
use rigging::{Stylize, widgets::Print};
use std::{net::TcpListener, time::Duration};

/// API: Handles server status checking.
pub async fn handle_server_status() -> Result<()> {
    let port = str!(Config::get().server.port);
    let client = Client::tcp();

    Print::h1("Checking server:").render().await?;

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
    Print::h1("Starting server:").render().await?;

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
pub async fn handle_server_stop(force: bool) -> Result<()> {
    Print::h1("Stopping server:").render().await?;

    let cfg = Config::get();
    let port = cfg.server.port;

    #[cfg(unix)]
    {
        let signal = if force { "-9" } else { "-15" };

        let _ = Command::new("fuser")
            .args(["-k", signal, &format!("{port}/tcp")])
            .output()
            .await;
    }

    #[cfg(windows)]
    {
        let force_flag = if force { "/f /t" } else { "/f" };
        let cmd = format!(
            "for /f \"tokens=5\" %a in ('netstat -aon ^| findstr \":{}\"') do taskkill {} /pid %a",
            port, force_flag
        );
        let _ = Command::new("cmd").args(["/C", &cmd]).output().await;
    }

    Print::field("Status", str!("Offline".red()))
        .render()
        .await?;

    let msg = if force {
        "Processes forcibly killed by port."
    } else {
        "Processes terminated by port."
    };

    Print::success(msg).margin_top(1).render().await?;
    println!();

    Ok(())
}

/// API: Handles server restarting.
pub async fn handle_server_restart(force: bool) -> Result<()> {
    handle_server_stop(force).await?;
    sleep(Duration::from_millis(500)).await;
    handle_server_start().await?;

    Ok(())
}
