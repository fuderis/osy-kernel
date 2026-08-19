use super::*;
use crate::prelude::*;

use osy_share::{AgentMetadata, StatusData};
use tokio::process::Command;

/// API: Handles the server refreshing (hot-reload)
pub async fn handle_refresh() -> Result<()> {
    let port = Settings::get().server.port;
    let client = Client::tcp();

    section("Refreshing Server");

    let res = client
        .get(&str!("http://127.0.0.1:{port}/refresh"))
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                info("Status", &str!("Online (port {port})").green().to_string());

                // successful response: we are parsing StatusData.
                let _data: StatusData = response
                    .json()
                    .await
                    .map_err(|e| str!("Failed to parse response: {e}"))?;

                success("Settings synchronized.");
            } else {
                // error 500 or another: read the error text from the body.
                let err_msg = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Unknown server error".to_string());

                error(format!("Server error ({}): {err_msg}", status).into());
            }
        }

        Err(_) => {
            info("Server status", &"Offline".red().to_string());
            warn("Server is not responding. Check if it's running.");
            return Err(str!("Server is offline").into());
        }
    }

    println!();
    Ok(())
}

/// API: Handles the server status checking
pub async fn handle_status() -> Result<()> {
    let port = Settings::get().server.port;
    let client = Client::tcp();

    section("Checking Server");

    // checking server:
    let res = client
        .get(&str!("http://127.0.0.1:{port}/status"))
        .send()
        .await;

    match res {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
                info("Status", &str!("Online (port {port})").green().to_string());

                // successful response: we are parsing StatusData.
                let data: StatusData = response
                    .json()
                    .await
                    .map_err(|e| str!("Failed to parse response: {e}"))?;

                info("Agents", "");

                if data.agents_list.is_empty() {
                    warn("No agents loaded");
                } else {
                    for AgentMetadata {
                        name, description, ..
                    } in data.agents_list
                    {
                        item(&name, &description.trim());
                    }
                }
            } else {
                // error 500 or another: read the error text from the body.
                let err_msg = response
                    .text()
                    .await
                    .unwrap_or_else(|_| "Failed to read error body".to_string());

                error(format!("Server error ({status}): {err_msg}").into());
            }
        }
        Err(_) => {
            info("Status", &"Offline".red().to_string());
        }
    }

    section("Checking LMS Server");

    // checking LMS server:
    let lms_out = Command::new("lms").args(["status"]).output().await;
    let lms_raw = match lms_out {
        Ok(out) => String::from_utf8_lossy(&out.stdout).to_string(),
        _ => String::new(),
    };

    let lms_running = lms_raw.contains("ON");
    let lms_port = lms_raw
        .lines()
        .find(|l| l.contains("port:"))
        .and_then(|l| l.split("port:").last())
        .map(|p| p.trim_matches(|c: char| !c.is_numeric()))
        .unwrap_or("unknown");

    if lms_running {
        info(
            "Status",
            &str!("Online (port {lms_port})").green().to_string(),
        );

        let mut in_models_block = false;
        let mut found_any = false;

        for line in lms_raw.lines() {
            let line = line.trim();
            if line.contains("Models") {
                in_models_block = true;
                continue;
            }

            if in_models_block && line.starts_with('·') {
                if !found_any {
                    info("Models", "");
                }

                found_any = true;
                let model_info = line.trim_start_matches('·').trim();
                if let Some((name, size)) = model_info.split_once(" - ") {
                    let short = name.rsplit('/').next().unwrap_or(name);
                    item("", &format!("{short} {}", size.dim()));
                } else {
                    item("", &model_info);
                }
            }
        }
        if !found_any && in_models_block {
            warn("No models currently loaded in LMS");
        }
    } else {
        info("Status", &"Offline".red().to_string());
    }

    println!();
    Ok(())
}

/// API: Opens the config in the default editor
pub async fn handle_config() -> Result<()> {
    let path = Settings::path();

    section("Configuration");
    info("Path", &path.display().to_string().white().to_string());

    #[cfg(target_os = "linux")]
    let opener = "xdg-open";

    #[cfg(target_os = "macos")]
    let opener = "open";

    #[cfg(any(target_os = "linux", target_os = "macos"))]
    match Command::new(opener).arg(&path).spawn() {
        Ok(_) => success("Config file opened in default editor."),
        Err(e) => error(str!("Failed to open config: {e}").into()),
    }

    println!();
    Ok(())
}
