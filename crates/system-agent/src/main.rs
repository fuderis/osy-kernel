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

pub mod handlers;
pub mod skills;
pub mod tools;

use clap::{Parser, Subcommand};
use pearce::Server;
use prelude::*;

pub const APP_NAME: &str = "ovsy-system-agent";
pub const APP_VERSION: &str = "0.3.0";

#[derive(Parser, Debug)]
#[command(name = APP_NAME, version = APP_VERSION, about = "Ovsy AI Agent Core System Component")]
struct Args {
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand, Debug)]
enum Commands {
    /// Prints agent metadata in JSON format and exits
    Metadata,
    /// Runs the AI agent server
    Serve,
}

#[tokio::main]
async fn main() -> Result<()> {
    use handlers as hands;

    // Parse CLI arguments
    let args = Args::parse();

    // init settings && logger:
    Settings::init(path!("$config$/config.toml")).await?;
    Logger::init(path!("$state$/logs"), Settings::get().server.max_logs).await?;

    // Handle subcommands
    match args.command {
        Commands::Metadata => {
            let metadata = &Settings::get().metadata;
            let json_output = serde_json::to_string(metadata)?;
            println!("{json_output}");
            Ok(())
        }

        Commands::Serve => {
            ovsy_share::macos_protect();

            // start server:
            let sock = path!("$temp/ovsy/uds/{}.sock", Settings::get().metadata.name);
            Server::new()
                //    HEALTH
                .get("/ping", hands::health::handle_ping)
                //    TOOLS
                .post("/tools/list", hands::tools::handle_tools_list)
                .post("/tools/call/{tool}", hands::tools::handle_tool_call)
                .run(sock)
                .await
        }
    }
}
