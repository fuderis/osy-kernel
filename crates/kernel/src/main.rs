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

pub mod context;
pub mod manager;
pub mod runtime;
pub mod session;

pub mod commands;
pub mod handlers;
pub mod skills;

pub mod chat;

use clap::{Parser, Subcommand};
use manager::Manager;
use pearce::Server;
use prelude::*;

pub const APP_NAME: &str = "ovsy";
pub const APP_VERSION: &str = "0.15.0";

/// The Ovsy CLI commands parser
#[derive(Parser)]
#[command(name = APP_NAME)]
#[command(version = APP_VERSION)]
#[command(about = "Ovsy — Ultra-Fast AI Kernel (experimental)", long_about = None)]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,
}

/// The Ovsy CLI commands
#[derive(Subcommand)]
enum Commands {
    /// Check the status of all ecosystem components
    Status,
    /// Refreshes the server settings & agents list
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
    /// Stop the Ovsy server by killing the port process
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
        Commands::Chat => cmds::chat::handle_chat().await,
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
        //    HEALTH
        .get("/ping", hands::health::handle_ping)
        .get("/status", hands::health::handle_status)
        .get("/refresh", hands::health::handle_refresh)
        //    USERS
        .post("/users/{uid}/sessions", hands::user::handle_list)
        //    SESSIONS
        .post("/sessions/{sid}/init", hands::session::handle_init)
        .post("/sessions/{sid}/finish", hands::session::handle_finish)
        .post("/sessions/{sid}/compact", hands::session::handle_compact)
        .post("/sessions/{sid}/clear", hands::session::handle_clear)
        //    QUERY
        .post("/sessions/{sid}/query", hands::query::handle_user_query)
        .run(Settings::get().server.port)
        .await?;

    Ok(())
}
