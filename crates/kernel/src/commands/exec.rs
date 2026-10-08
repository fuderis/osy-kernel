use crate::{prelude::*, utils};

use anylm::api::Message;
use osy_share::{HandleQuery, SessionId, ToolQuery};

/// API: Handling specific skill tool.
pub async fn handle_tool_call(skill_name: &str, tool_name: &str, payload: String) -> Result<()> {
    let payload_trim = payload.trim();

    // parsing payload
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
    let url = format!("{base_url}/skills/{skill_name}/call/{tool_name}");
    utils::render_response(
        url,
        ToolQuery {
            current_path: std::env::current_dir()
                .map_err(|e| {
                    warn!("Failed to get current_dir: {e}");
                    e
                })
                .ok(),
            payload: json_payload,
        },
    )
    .await
}

/// API: Handling skill with a text request.
pub async fn handle_skill_query(
    uid: u64,
    sid: Option<SessionId>,
    new_session: bool,
    load_history: bool,
    skill_name: String,
    query: String,
) -> Result<()> {
    let port = Config::get().server.port;
    let base_url = format!("http://127.0.0.1:{port}");
    let client = Client::tcp();

    // refresh/start kernel server
    utils::ensure_server(&client, &base_url).await?;

    // select session or create new
    let sid = if new_session || uid == 0 {
        SessionId::new(uid)
    } else {
        if let Some(session_id) = sid {
            session_id
        } else {
            utils::select_session(&client, &base_url, uid)
                .await?
                .unwrap_or(SessionId::new(uid))
        }
    };

    // init session handling
    utils::init_session(&client, &base_url, sid, load_history).await?;

    // rendering response
    let url = format!("{base_url}/sessions/{sid}/skills/{skill_name}/query");
    utils::render_response(
        url,
        HandleQuery {
            current_path: std::env::current_dir()
                .map_err(|e| {
                    warn!("Failed to get current_dir: {e}");
                    e
                })
                .ok(),
            message: Message::user(vec![query.into()]),
        },
    )
    .await
}
