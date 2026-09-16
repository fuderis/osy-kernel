// Copyright (C) 2026 Bulat Sh. (fuderis) <synapdrake@ya.ru>
//
// This program is free software: you can redistribute it and/or modify
// it under the terms of the GNU General Public License as published by
// the Free Software Foundation, either version 3 of the License, or
// (at your option) any later version.
//
// This program is distributed in the hope that it will be useful,
// but WITHOUT ANY WARRANTY; without even the implied warranty of
// MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE. See the
// GNU General Public License for more details.
//
// You should have received a copy of the GNU General Public License
// along with this program. If not, see <https://www.gnu.org/licenses/>.

pub mod error;
pub mod prelude;

pub mod settings;
pub mod utils;

pub mod manager;
pub mod runtime;
pub mod user;

pub mod commands;
pub mod handlers;
pub mod skills;

use manager::Manager;
use pearce::Server;
use prelude::*;

use rigging::{CommandContext, Commands, Stylize, pkg_meta};

#[tokio::main]
async fn main() -> Result<()> {
    use commands as cmds;

    // init settings
    Settings::init(path!("$config$/settings.toml")).await?;

    // handle arguments
    if let Err(e) = Commands::new()
        .meta(pkg_meta!())
        .hide_cmd("serve", "Serve the kernel server (internal).", serve)
        //    SERVER
        .group("server", "Server management commands.")
        .cmd(
            "server status",
            "Check the status of kernel server.",
            |_| async move { cmds::server::handle_status().await },
        )
        .cmd(
            "server start -l|--lms=false",
            "Start the kernel server in the background.",
            |ctx| async move {
                let start_lms = ctx.get("lms")?;
                cmds::server::handle_start(start_lms).await
            },
        )
        .cmd(
            "server stop -l|--lms=false",
            "Stop the server by killing the port process.",
            |ctx| async move {
                let stop_lms = ctx.get("lms")?;
                cmds::server::handle_stop(stop_lms).await
            },
        )
        .cmd(
            "server restart -l|--lms=false",
            "Restart the ecosystem (stop -> start).",
            |ctx| async move {
                let restart_lms = ctx.get("lms")?;
                cmds::server::handle_restart(restart_lms).await
            },
        )
        //    HEALTH
        .cmd(
            "status",
            "Check the status of all ecosystem components.",
            |_| async move { cmds::health::handle_status().await },
        )
        .cmd(
            "refresh",
            "Refresh the server settings & agents list.",
            |_| async move { cmds::health::handle_refresh().await },
        )
        .cmd(
            "config",
            "Open settings.toml in the default system editor.",
            |_| async move { cmds::health::handle_config().await },
        )
        //    CHAT
        .cmd(
            "chat -u|--uid=1 -n|--new=false -l|--load=false -s|--sudo=false",
            "Enter interactive AI chat mode.",
            |ctx| async move {
                let uid = ctx.get("uid")?;
                let new_session = ctx.get("new")?;
                let load_history = ctx.get("load")?;
                let use_sudo = ctx.get("sudo")?;
                cmds::chat::handle_chat(uid, new_session, load_history, use_sudo).await
            },
        )
        //    TRACING
        .cmd(
            "trace -u|--uid= -n|--new=true",
            "Trace live ecosystem log files dynamically.",
            |ctx| async move {
                let uid = ctx.get_opt::<u64>("uid")?;
                let only_new = ctx.get("new")?;
                cmds::trace::handle_trace(uid, only_new).await
            },
        )
        .run()
        .await
    {
        eprintln!("{} {e}", "Error:".red().bold());
    }

    Ok(())
}

/// API: Handles server launching (inline).
async fn serve(_: CommandContext) -> Result<()> {
    use crate::handlers as hands;

    // init logger & agents management
    Logger::init(path!("$state$/logs"), 1000).await?;
    Manager::init().await?;

    // start server
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
        .callback(true)
        .run(Settings::get().server.port)
        .await?;

    Ok(())
}
