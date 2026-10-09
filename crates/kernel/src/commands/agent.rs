use crate::prelude::*;

use atoman::{Command, fs};
use heck::ToPascalCase;
use osy_share::StatusData;
use rigging::{Stylize, widgets::Print};

/// API: Handles agent list command.
pub async fn handle_agent_list() -> Result<()> {
    let port = str!(Config::get().server.port);
    let client = Client::tcp();

    Print::h1("Receiving agents list:")
        .margin_bottom(1)
        .render()
        .await?;

    let response = client
        .get(&format!("http://127.0.0.1:{port}/status"))
        .send()
        .await
        .map_err(|e| format!("Failed to reach server: {e}"))?;

    if !response.status().is_success() {
        let err_msg = response
            .text()
            .await
            .unwrap_or_else(|_| str!("Failed to read error body"));
        return Err(format!("Server returned error: {err_msg}").into());
    }

    let data: StatusData = response
        .json()
        .await
        .map_err(|e| format!("Failed to parse response: {e}"))?;

    if data.agents_list.is_empty() {
        Print::info("No agents loaded.").render().await?;
        return Ok(());
    }

    for agent in data.agents_list {
        let mut block = Print::h2(
            format!("{} agent", agent.name.to_pascal_case())
                .bold()
                .to_string(),
        )
        .field("Name", agent.name.trim())
        .field("Description", agent.description.trim())
        .field("Version", &agent.version)
        .field("Socket Name", &agent.sock_name)
        .field("Socket Path", agent.sock_path.display().to_string());

        if !agent.skills.is_empty() {
            block = block.field("Skills", "");
            for skill in agent.skills.values() {
                block = block.tree_item(format!(
                    "{} — {}",
                    skill.name.as_str().bold(),
                    skill.description
                ));
            }
        } else {
            block = block.field("Skills", "None");
        }

        block.margin_bottom(1).render().await?;
    }

    Ok(())
}

/// API: Handles new agent creation from template.
pub async fn handle_agent_new(name: String, descr: Option<String>) -> Result<()> {
    Print::h1("Creating new agent:").render().await?;

    let templ_path = "https://github.com/fuderis/osy-agent.git";
    let folder_name = if name.starts_with("osy-") {
        name
    } else {
        format!("osy-{name}")
    };

    let process = async {
        // cloning repository
        Print::info(format!("Cloning into {folder_name}..."))
            .margin_left(1)
            .render()
            .await?;

        let output = Command::new("git")
            .args(["clone", templ_path, &folder_name])
            .output()
            .await
            .map_err(|e| format!("Failed to execute git clone: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git clone failed: {}", stderr.trim()).into());
        }

        // removing old .git folder
        let git_dir = std::path::Path::new(&folder_name).join(".git");
        if git_dir.exists() {
            Print::info("Cleaning up git metadata...")
                .margin_left(1)
                .render()
                .await?;

            fs::remove_dir_all(&git_dir)
                .await
                .map_err(|e| format!("Failed to remove old .git directory: {e}"))?;
        }

        // init new .git index
        Print::info("Initializing new git repository...")
            .margin_left(1)
            .render()
            .await?;

        let output = Command::new("git")
            .arg("init")
            .current_dir(&folder_name)
            .output()
            .await
            .map_err(|e| format!("Failed to execute git init: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git init failed: {}", stderr.trim()).into());
        }

        // editing cargo configuration
        Print::info("Updating Cargo configuration...")
            .margin_left(1)
            .render()
            .await?;

        let cargo_path = std::path::Path::new(&folder_name).join("Cargo.toml");
        let content = fs::read_to_string(&cargo_path)
            .await
            .map_err(|e| format!("Failed to read Cargo.toml: {e}"))?;

        let mut lines: Vec<String> = content.lines().map(String::from).collect();
        let mut in_package_section = false;
        let mut descr_updated = false;

        for line in lines.iter_mut() {
            let trimmed = line.trim();

            if trimmed.starts_with('[') {
                in_package_section = trimmed == "[package]";
                continue;
            }

            if in_package_section {
                if trimmed.starts_with("name ") || trimmed.starts_with("name=") {
                    *line = format!("name = \"{folder_name}\"");
                } else if let Some(ref description) = descr {
                    if trimmed.starts_with("description ") || trimmed.starts_with("description=") {
                        *line = format!("description = \"{description}\"");
                        descr_updated = true;
                    }
                }
            }
        }

        if let Some(ref description) = descr {
            if !descr_updated {
                if let Some(pkg_idx) = lines.iter().position(|l| l.trim() == "[package]") {
                    lines.insert(pkg_idx + 1, format!("description = \"{description}\""));
                }
            }
        }

        let new_content = lines.join("\n") + "\n";
        fs::write(&cargo_path, new_content)
            .await
            .map_err(|e| format!("Failed to update Cargo.toml: {e}"))?;

        Print::success("Ready!").margin_left(1).render().await?;

        Ok::<_, DynError>(())
    };

    match process.await {
        Ok(()) => {
            Print::success(format!("Created agent {folder_name}."))
                .margin_top(1)
                .render()
                .await?;
            Ok(())
        }
        Err(e) => Err(Error::Titled("Failed to create agent".into(), e.into()).into()),
    }
}
