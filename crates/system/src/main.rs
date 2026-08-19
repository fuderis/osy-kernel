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

use clap::{Parser, Subcommand};
use pearce::Server;
use prelude::*;

#[derive(Parser, Debug)]
#[command(name = env!("CARGO_PKG_NAME"))]
#[command(version = env!("CARGO_PKG_VERSION"))]
#[command(about = env!("CARGO_PKG_DESCRIPTION"))]
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
            let metadata = osy_share::agent_metadata!();
            let json_output = serde_json::to_string(&metadata)?;
            println!("{json_output}");
            Ok(())
        }

        Commands::Serve => {
            osy_share::macos_protect();

            // start server:
            let sock = path!(
                "$temp/osy/socks/{}.sock",
                env!("CARGO_PKG_NAME").trim_start_matches("osy-")
            );

            info!("Launching on `{}`...", sock.display());
            Server::new()
                //    HEALTH
                .get("/ping", hands::health::handle_ping)
                //    SKILLS
                .post("/skills/list", hands::skills::handle_skills_list)
                .post("/skills/{skill}/tools", hands::skills::handle_tools_list)
                .post(
                    "/skills/{skill}/call/{tool}",
                    hands::skills::handle_tool_call,
                )
                .run(sock)
                .await
        }
    }
}
