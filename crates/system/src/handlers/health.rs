use crate::prelude::*;

/// API: Handles the server ping
pub async fn handle_ping() -> Response {
    Response::ok().text("pong")
}
