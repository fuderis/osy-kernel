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
pub mod helpers;
pub mod prelude;
pub mod settings;

pub mod context;
pub mod manager;
pub mod runtime;
pub mod user;

pub mod commands;
pub mod handlers;
pub mod skills;

use clap::{Parser, Subcommand};
use manager::Manager;
use pearce::Server;
use prelude::*;

pub const APP_NAME: &str = "osy";

/// The CLI commands parser
#[derive(Parser)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = env!("CARGO_PKG_DESCRIPTION"), long_about = None)]
struct Cli {
    /// Load session history on startup when entering interactive chat mode
    #[arg(short, long)]
    load: bool,

    #[command(subcommand)]
    command: Option<Commands>,
}

/// The CLI commands
#[derive(Subcommand)]
enum Commands {
    /// Check the status of all ecosystem components
    Status,
    /// Refresh the server settings & agents list
    Refresh,

    /// Serve the kernel server
    #[command(hide = true)]
    Serve,
    /// Start the kernel server in the background
    Start {
        /// Also run the LM Studio server and load models
        #[arg(short, long)]
        lms: bool,
    },
    /// Stop the server by killing the port process
    Stop {
        /// Also stop the LM Studio server and unload models
        #[arg(short, long)]
        lms: bool,
    },
    /// Restart the ecosystem (stop -> start)
    Restart {
        #[arg(short, long)]
        lms: bool,
    },

    /// Enter interactive AI chat mode
    Chat,

    /// Open settings.toml in the default system editor
    #[command(alias = "conf")]
    Config,
}

#[tokio::main]
async fn main() -> Result<()> {
    use commands as cmds;

    // parse arguments:
    let cli = Cli::parse();

    // init settings:
    Settings::init(path!("$config$/settings.toml")).await?;

    if let Err(e) = match cli.command.unwrap_or(Commands::Chat) {
        //     SYSTEM
        Commands::Serve => serve().await,
        Commands::Start { lms } => cmds::server::handle_start(lms).await,
        Commands::Stop { lms } => cmds::server::handle_stop(lms).await,
        Commands::Restart { lms } => cmds::server::handle_restart(lms).await,

        //     HEALTH
        Commands::Status => cmds::health::handle_status().await,
        Commands::Refresh => cmds::health::handle_refresh().await,
        Commands::Config => cmds::health::handle_config().await,

        //     CHAT
        Commands::Chat => cmds::chat::handle_chat(cli.load).await,
    } {
        cmds::error(e);
        std::process::exit(1);
    }

    Ok(())
}

async fn serve() -> Result<()> {
    use handlers as hands;

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
        //      USER FACTS (RAG)
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
        //      GLOBAL USER RULES
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
        //      LOCAL SESSION RULES
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
