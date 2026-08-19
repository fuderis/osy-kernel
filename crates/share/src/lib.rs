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

pub mod agent_metadata;
pub use agent_metadata::*;

pub mod session_id;
pub use session_id::*;

pub mod session_info;
pub use session_info::*;

pub mod skill;
pub use skill::*;

pub mod fact;
pub use fact::*;

pub mod rule;
pub use rule::*;

pub mod event;
pub use event::*;

pub mod query;
pub use query::*;

pub mod data;
pub use data::*;

pub fn macos_protect() {
    #[cfg(target_os = "macos")]
    {
        tokio::spawn(async {
            use tokio::io::AsyncReadExt;
            let mut std_in = tokio::io::stdin();
            let mut buf = [0; 1];
            if let Ok(0) = std_in.read(&mut buf).await {
                std::process::exit(0);
            }
        });
    }
}
