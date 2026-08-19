use crate::{Manager, prelude::*};
use osy_share::StatusData;

/// API: Handles the server ping
pub async fn handle_ping() -> Response {
    Response::ok().text("pong")
}

/// Returns the server status & agents list
pub async fn handle_status() -> Response {
    let agents_list = Manager::agents_list().await;
    Response::ok().json(&StatusData { agents_list })
}

/// Refreshes the server settings & agents list
pub async fn handle_refresh() -> Response {
    // update settings:
    if let Err(e) = Settings::update().await {
        return Response::error().text(str!("{e}"));
    }

    // update agents:
    if let Err(e) = Manager::update().await {
        return Response::error().text(str!("{e}"));
    }

    let agents_list = Manager::agents_list().await;
    Response::ok().json(&StatusData { agents_list })
}
