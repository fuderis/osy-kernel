use crate::{prelude::*, utils};

use anylm::api::Message;
use osy_share::HandleQuery;

/// API: Handling specific skill tool.
pub async fn handle_tool_call(skill_name: &str, tool_name: &str, payload: String) -> Result<()> {
    let payload_trim = payload.trim();

    // 1. Парсим payload
    let json_payload = if payload_trim.starts_with('{') || payload_trim.starts_with('[') {
        serde_json::from_str::<JsonValue>(payload_trim)
            .unwrap_or_else(|_| JsonValue::String(payload.clone()))
    } else {
        utils::parse_cli_kv(payload_trim)
    };

    let port = Config::get().server.port;
    let base_url = format!("http://127.0.0.1:{port}");
    let client = Client::tcp();

    // refresh/start kernel server
    utils::ensure_server(&client, &base_url).await?;

    // rendering response
    let url = format!("http://127.0.0.1:{port}/skills/{skill_name}/call/{tool_name}");
    utils::render_response(url, json_payload).await
}

/// API: Handling skill with a text request.
pub async fn handle_skill_query(skill_name: String, query: String) -> Result<()> {
    let query_payload = HandleQuery {
        message: Message::user(vec![query.into()]),
        info: Some(utils::session_info()),
    };

    let port = Config::get().server.port;
    let base_url = format!("http://127.0.0.1:{port}");
    let client = Client::tcp();

    // refresh/start kernel server
    utils::ensure_server(&client, &base_url).await?;

    // rendering response
    let url = format!("http://127.0.0.1:{port}/skills/{skill_name}/query");
    utils::render_response(url, query_payload).await
}
