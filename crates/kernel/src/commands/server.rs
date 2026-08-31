use super::*;
use crate::{Manager, prelude::*};

use osy_share::{AgentMetadata, StatusData};
use pearce::Server;
use std::{net::TcpListener, process::Stdio, time::Duration};
use tokio::{
    process::Command,
    time::{sleep, timeout},
};

const TIMEOUT: Duration = Duration::from_millis(500);

/// API: Handles the server launching (inline)
pub async fn handle_serve() -> Result<()> {
    use crate::handlers as hands;

    // init logger & agents manager:
    Logger::init(path!("$state$/logs"), Settings::get().server.max_logs).await?;
    Manager::init().await?;

    // start server:
    Server::new()
        //      HEALTH
        .get("/ping", hands::health::handle_ping)
        .get("/status", hands::health::handle_status)
        .get("/refresh", hands::health::handle_refresh)
        //      USERS
        .post("/users/{uid}/sessions", hands::users::handle_list)
        .post("/users/{uid}/facts/list", hands::users::handle_facts_list)
        .post("/users/{uid}/facts/set", hands::users::handle_facts_set)
        .post(
            "/users/{uid}/facts/remove",
            hands::users::handle_facts_remove,
        )
        .post("/users/{uid}/facts/clear", hands::users::handle_facts_clear)
        .post(
            "/users/{uid}/facts/search",
            hands::users::handle_facts_search,
        )
        .post("/users/{uid}/rules/list", hands::users::handle_rules_list)
        .post("/users/{uid}/rules/set", hands::users::handle_rules_set)
        .post(
            "/users/{uid}/rules/remove",
            hands::users::handle_rules_remove,
        )
        .post("/users/{uid}/rules/clear", hands::users::handle_rules_clear)
        //      SESSIONS
        .post("/sessions/{sid}/init", hands::sessions::handle_init)
        .post("/sessions/{sid}/finish", hands::sessions::handle_finish)
        .post("/sessions/{sid}/compact", hands::sessions::handle_compact)
        .post("/sessions/{sid}/clear", hands::sessions::handle_clear)
        .post("/sessions/{sid}/clone", hands::sessions::handle_clone)
        .post(
            "/sessions/{sid}/rules/list",
            hands::sessions::handle_rules_list,
        )
        .post(
            "/sessions/{sid}/rules/set",
            hands::sessions::handle_rules_set,
        )
        .post(
            "/sessions/{sid}/rules/remove",
            hands::sessions::handle_rules_remove,
        )
        .post(
            "/sessions/{sid}/rules/clear",
            hands::sessions::handle_rules_clear,
        )
        //      QUERY
        .post("/sessions/{sid}/query", hands::query::handle_user_query)
        .run(Settings::get().server.port)
        .await?;

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

    println!();
    Ok(())
}

/// API: Handles the server launching
pub async fn handle_start(start_lms: bool) -> Result<()> {
    section("Starting Services");

    let cfg = Settings::get();

    // 1. Start Server
    let port = cfg.server.port;
    let is_port_free = TcpListener::bind(str!("127.0.0.1:{port}")).is_ok();

    if is_port_free {
        Command::new(path!("$"))
            .args(&["server", "serve"])
            .current_dir(path!("$/"))
            .kill_on_drop(false)
            .spawn()?;
        info("Osy Server", &"Online".green().to_string());
    } else {
        warn(&format!("Osy Server: Port {port} is already in use"));
    }

    // 2. Start LMS Server
    if start_lms {
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

                    // 100 tries * 100 ms = 10 seconds to start:
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
                        info("LMS Server", &"Online".green().to_string());
                    } else {
                        info("LMS Server", &"Failed to start".red().to_string());
                    }
                }
                Err(e) => {
                    error(format!("Failed to spawn LMS process: {e}").into());
                    info("LMS Server", &"Failed".red().to_string());
                }
            }
        } else {
            info("LMS Server", &"Online".green().to_string());
        }
    }

    success("Ready for requests!");
    println!();

    Ok(())
}

/// API: Handles the server shutdown
pub async fn handle_stop(stop_lms: bool) -> Result<()> {
    section("Stopping Services");

    let cfg = Settings::get();
    let port = cfg.server.port;

    // 1. Stop server
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
    info("Osy Server", &"Offline".red().to_string());

    // 2. Stop LMS server
    if stop_lms {
        // Unload models first
        let _ = timeout(
            TIMEOUT,
            Command::new("lms").args(["unload", "--all"]).output(),
        )
        .await;
        info("LMS Models", &"Unloaded".red().to_string());

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
        info("LMS Server", &"Offline".red().to_string());
    }

    success("Processes terminated.");
    println!();

    Ok(())
}

/// API: Handles the server restarting
pub async fn handle_restart(restart_lms: bool) -> Result<()> {
    handle_stop(restart_lms).await?;
    sleep(Duration::from_millis(800)).await;
    handle_start(restart_lms).await?;

    Ok(())
}
