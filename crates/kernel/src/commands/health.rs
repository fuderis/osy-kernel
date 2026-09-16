use crate::prelude::*;

use osy_share::{AgentMeta, StatusData};
use rigging::{Stylize, widgets::Print};
use tokio::process::Command;

/// API: Handles server hot-reload.
pub async fn handle_refresh() -> Result<()> {
    let port = str!(Settings::get().server.port);
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

    Print::h1("LM Studio Server:")
        .margin_top(1)
        .render()
        .await?;

    // checking LMS server
    let lms_raw = match Command::new("lms").args(["status"]).output().await {
        Ok(out) => str!(String::from_utf8_lossy(&out.stdout)),
        _ => str!(),
    };

    let lms_running = lms_raw.contains("ON");
    let lms_port = lms_raw
        .lines()
        .find(|l| l.contains("port:"))
        .and_then(|l| l.split("port:").last())
        .map(|p| p.trim_matches(|c: char| !c.is_numeric()))
        .unwrap_or("unknown");

    if lms_running {
        let fields = Print::new()
            .field("Status", str!(format!("Online").green()))
            .field("Port", str!(lms_port.green()));

        let mut in_models_block = false;
        let mut found_any = false;

        let mut models = Print::new();

        for line in lms_raw.lines() {
            let line = line.trim();
            if line.contains("Models") {
                in_models_block = true;
                continue;
            }

            if in_models_block && line.starts_with('·') {
                found_any = true;
                let model_info = line.trim_start_matches('·').trim();
                if let Some((name, size)) = model_info.split_once(" - ") {
                    let short = name.rsplit('/').next().unwrap_or(name);
                    models = models.tree_item(format!("{short} {}", size.dim()));
                } else {
                    models = models.tree_item(model_info);
                }
            }
        }

        fields.field("Models", "").render().await?;

        if !found_any {
            models = models.tree_item("No models loaded.");
        }
        models.render().await?;
    } else {
        Print::field("Status", str!("Offline".red()))
            .render()
            .await?;
    }

    println!();
    Ok(())
}

/// API: Opens config in the default editor.
pub async fn handle_config() -> Result<()> {
    let path = Settings::path();

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
