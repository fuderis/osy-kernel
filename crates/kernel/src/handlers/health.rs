use crate::{Manager, prelude::*};
use osy_share::StatusData;

/// API: Handles the server ping.
#[log()]
pub async fn handle_ping() -> Response {
    Response::ok().text("pong")
}

/// API: Returns the server status & agents list.
#[log()]
pub async fn handle_status() -> Response {
    let agents_list = Manager::agents_list().await;
    Response::ok().json(&StatusData { agents_list })
}

/// API: Refreshes the server settings & agents list.
#[log()]
pub async fn handle_refresh() -> Response {
    // update settings
    if let Err(e) = Config::update().await {
        return Response::error().text(e.to_string());
    }

    // update agents
    if let Err(e) = Manager::update().await {
        return Response::error().text(e.to_string());
    }

    let agents_list = Manager::agents_list().await;
    Response::ok().json(&StatusData { agents_list })
}

/// API: Returns configured AI provider options.
#[log()]
pub async fn handle_options() -> Response {
    Response::ok().json(&Config::get().completions)
}
