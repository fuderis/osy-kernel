use crate::prelude::*;

use atoman::{io::AsyncReadExt, process::Command, time::sleep};
use osy_share::{AgentMeta, StatusData};
use rigging::{Stylize, widgets::Print};
use std::{net::TcpListener, process::Stdio, time::Duration};

/// API: Handles server status checking.
pub async fn handle_server_status() -> Result<()> {
    let port = str!(Config::get().server.port);
    let client = Client::tcp();

    Print::h1("Checking server:").render().await?;

    if !ping_server(&client, &port).await {
        Print::field("Status", str!("Offline".red()))
            .render()
            .await?;
        println!();
        return Ok(());
    }

    Print::new()
        .field("Status", str!("Online".green()))
        .field("Port", str!(port.as_str().green()))
        .render()
        .await?;

    match client
        .get(&format!("http://127.0.0.1:{port}/status"))
        .send()
        .await
    {
        Ok(response) => {
            let status = response.status();
            if status.is_success() {
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

                Print::error(format!("Server status error ({status}): {err_msg}"))
                    .margin_top(1)
                    .render()
                    .await?;
            }
        }
        Err(e) => {
            Print::error(format!("Failed to fetch detailed status: {e}"))
                .margin_top(1)
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
    let port = str!(cfg.server.port);
    let client = Client::tcp();

    let is_port_free = TcpListener::bind(format!("127.0.0.1:{port}")).is_ok();

    if is_port_free {
        // spawning server process
        let mut child = Command::new(path!("$"))
            .arg("serve")
            .current_dir(path!("$/"))
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(false)
            .spawn()?;

        // waiting server ping
        match wait_for_server_start(&mut child, &client, &port, 30, Duration::from_millis(500))
            .await
        {
            Ok(()) => {
                Print::new()
                    .field("Status", str!("Online".green()))
                    .field("Port", str!(port.green()))
                    .render()
                    .await?;

                Print::success("Ready for requests!")
                    .margin_top(1)
                    .render()
                    .await?;
            }
            Err(e) => {
                Print::error(e.to_string()).margin_top(1).render().await?;
            }
        }
    } else {
        if ping_server(&client, &port).await {
            Print::warn(format!("Server is already running on port {port}."))
                .render()
                .await?;
            Print::new()
                .field("Status", str!("Online".green()))
                .field("Port", str!(port.green()))
                .render()
                .await?;
        } else {
            Print::warn(format!(
                "Port {port} is already in use by another process..."
            ))
            .render()
            .await?;
        }
    }

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
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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
        let _ = Command::new("cmd")
            .args(["/C", &cmd])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .output()
            .await;
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

async fn ping_server(client: &Client, port: &str) -> bool {
    client
        .get(&format!("http://127.0.0.1:{port}/ping"))
        .send()
        .await
        .map(|res| res.status().is_success())
        .unwrap_or(false)
}

async fn wait_for_server_start(
    child: &mut atoman::process::Child,
    client: &Client,
    port: &str,
    retries: usize,
    delay: Duration,
) -> Result<()> {
    for _ in 1..=retries {
        // check whether child process has terminated prematurely
        if let Ok(Some(status)) = child.try_wait() {
            let mut combined_output = String::new();

            // read stdout
            if let Some(mut stdout) = child.stdout.take() {
                let _ = stdout.read_to_string(&mut combined_output).await;
            }

            // read stderr
            if let Some(mut stderr) = child.stderr.take() {
                let mut err_str = String::new();
                if stderr.read_to_string(&mut err_str).await.is_ok() && !err_str.is_empty() {
                    if !combined_output.is_empty() {
                        combined_output.push('\n');
                    }
                    combined_output.push_str(&err_str);
                }
            }

            let error_message = combined_output.trim();
            if !error_message.is_empty() {
                return Err(
                    Error::Custom(format!("Server failed to start:\n{error_message}")).into(),
                );
            } else {
                return Err(Error::Custom(format!(
                    "Process exited prematurely with status: {status}"
                ))
                .into());
            }
        }

        // check whether server has started and responds to /ping
        if ping_server(client, port).await {
            return Ok(());
        }

        // delay between attempts
        sleep(delay).await;
    }

    Err(Error::Custom(format!(
        "Server process spawned, but failed to respond to /ping after {retries} attempts."
    ))
    .into())
}
