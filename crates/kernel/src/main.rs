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

pub mod config;
pub mod error;
pub mod prelude;
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

#[atoman::main]
async fn main() -> Result<()> {
    use commands as cmds;

    // init config
    Config::init(path!("$config$/config.toml")).await?;

    // handle arguments
    if let Err(e) = Commands::new()
        .meta(pkg_meta!())
        .hide_cmd("serve", "Serve the kernel server (internal).", serve)
        //    SERVER
        .group("server", "Server management commands.")
        .cmd(
            "server status",
            "Check the status of kernel server.",
            |_| async move { cmds::handle_server_status().await },
        )
        .cmd(
            "server start",
            "Start the kernel server in the background.",
            |_| async move { cmds::handle_server_start().await },
        )
        .cmd(
            "server stop",
            "Stop the server by killing the port process.",
            |_| async move { cmds::handle_server_stop().await },
        )
        .cmd(
            "server restart",
            "Restart the ecosystem (stop -> start).",
            |_| async move { cmds::handle_server_restart().await },
        )
        //    HEALTH
        .cmd(
            "status",
            "Check the status of all ecosystem components.",
            |_| async move { cmds::handle_health_status().await },
        )
        .cmd(
            "refresh",
            "Refresh the server settings & agents list.",
            |_| async move { cmds::handle_health_refresh().await },
        )
        .cmd(
            "config",
            "Open settings.toml in the default system editor.",
            |_| async move { cmds::handle_health_config().await },
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
                cmds::handle_chat(uid, new_session, load_history, use_sudo).await
            },
        )
        //    TRACING
        .cmd(
            "trace -u|--uid= -n|--new=true",
            "Trace live ecosystem log files dynamically.",
            |ctx| async move {
                let uid = ctx.get_opt::<u64>("uid")?;
                let only_new = ctx.get("new")?;
                cmds::handle_trace(uid, only_new).await
            },
        )
        //    SKILLS
        .cmd(
            "do {skill} {payload..}",
            "Executes agent skills directly.",
            |ctx| async move {
                let skill = ctx.get::<String>("skill")?;
                let payload = ctx.get::<String>("payload")?;
                match skill.split_once('.') {
                    Some((skill_name, tool_name)) => {
                        cmds::handle_tool_call(skill_name, tool_name, payload).await
                    }
                    None => cmds::handle_skill_query(skill, payload).await,
                }
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
        .get("/ping", hands::handle_ping)
        .get("/status", hands::handle_status)
        .get("/refresh", hands::handle_refresh)
        .get("/options", hands::handle_options)
        //      USERS
        .post("/users/{uid}/sessions", hands::handle_user_sessions_list)
        .post("/users/{uid}/facts/list", hands::handle_user_facts_list)
        .post("/users/{uid}/facts/set", hands::handle_user_facts_set)
        .post("/users/{uid}/facts/remove", hands::handle_user_facts_remove)
        .post("/users/{uid}/facts/clear", hands::handle_user_facts_clear)
        .post("/users/{uid}/facts/search", hands::handle_user_facts_search)
        .post("/users/{uid}/rules/list", hands::handle_user_rules_list)
        .post("/users/{uid}/rules/set", hands::handle_user_rules_set)
        .post("/users/{uid}/rules/remove", hands::handle_user_rules_remove)
        .post("/users/{uid}/rules/clear", hands::handle_user_rules_clear)
        //      SESSIONS
        .post("/sessions/{sid}/init", hands::handle_session_init)
        .post("/sessions/{sid}/finish", hands::handle_session_finish)
        .post("/sessions/{sid}/compact", hands::handle_session_compact)
        .post("/sessions/{sid}/clear", hands::handle_session_clear)
        .post("/sessions/{sid}/clone", hands::handle_session_clone)
        .post(
            "/sessions/{sid}/rules/list",
            hands::handle_session_rules_list,
        )
        .post("/sessions/{sid}/rules/set", hands::handle_session_rules_set)
        .post(
            "/sessions/{sid}/rules/remove",
            hands::handle_session_rules_remove,
        )
        .post(
            "/sessions/{sid}/rules/clear",
            hands::handle_session_rules_clear,
        )
        //      QUERY
        .post("/sessions/{sid}/query", hands::handle_user_query)
        //      SKILLS
        .post("/skills/{skill}/query", hands::handle_skill_query)
        .post("/skills/{skill}/call/{tool}", hands::handle_tool_call)
        .callback(true)
        .run(Config::get().server.port)
        .await?;

    Ok(())
}
