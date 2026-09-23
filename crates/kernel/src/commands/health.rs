use crate::prelude::*;

use atoman::process::Command;
use osy_share::{AgentMeta, StatusData};
use rigging::{Stylize, widgets::Print};

/// API: Handles server hot-reload.
pub async fn handle_health_refresh() -> Result<()> {
    let port = str!(Config::get().server.port);
    let client = Client::tcp();

    Print::h1("Kernel Server:").render().await?;

    // refreshing server
    match client
        .get(&format!("http://127.0.0.1:{port}/refresh"))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                Print::field("Status", str!("Online".green()))
                    .field("Port", str!(port.green()))
                    .render()
                    .await?;

                let _data: StatusData = response
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse response: {e}"))?;

                Print::success("Settings synchronized.")
                    .margin_top(1)
                    .render()
                    .await?;
            } else {
                let err_msg = response
                    .text()
                    .await
                    .unwrap_or_else(|_| str!("Unknown server error"));

                Print::error(format!("Server error ({}): {err_msg}", status))
                    .margin_top(1)
                    .render()
                    .await?;
            }
        }

        Err(_) => {
            Print::field("Server status", str!("Offline".red()))
                .render()
                .await?;

            Print::warn("Server is not responding. Check if it's running.")
                .margin_ver(1)
                .render()
                .await?;

            return Err(Error::Custom("Server is offline".into()).into());
        }
    }

    println!();
    Ok(())
}

/// API: Handles server status checking.
pub async fn handle_health_status() -> Result<()> {
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
                Print::field("Status", str!("Online".green()))
                    .field("Port", str!(port.green()))
                    .render()
                    .await?;

                let data: StatusData = response
                    .json()
                    .await
                    .map_err(|e| format!("Failed to parse response: {e}"))?;

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

/// API: Opens config in the default editor.
pub async fn handle_health_config() -> Result<()> {
    let path = Config::path();

    Print::h1("Configuration:").render().await?;
    Print::field("Path", str!(path.to_string_lossy().magenta()))
        .render()
        .await?;

    #[cfg(target_os = "linux")]
    let opener = "xdg-open";

    #[cfg(target_os = "macos")]
    let opener = "open";

    #[cfg(target_os = "windows")]
    let opener = "explorer";

    #[cfg(any(target_os = "linux", target_os = "macos", target_os = "windows"))]
    match Command::new(opener).arg(&path).spawn() {
        Ok(_) => {
            Print::success("Config file opened in default editor.")
                .margin_top(1)
                .render()
                .await?
        }

        Err(e) => {
            Print::error(format!("Failed to open config: {e}"))
                .margin_top(1)
                .render()
                .await?
        }
    }

    println!();
    Ok(())
}
