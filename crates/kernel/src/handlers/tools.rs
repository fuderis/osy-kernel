use crate::{Manager, prelude::*};

use anylm::api::{ToolCall, ToolCallFunction};
use osy_share::Event;
use pearce::Callback;

/// API: Handles skill tool call.
pub async fn handle_tool_call(
    skill_and_tool: Paths<(String, String)>,
    payload: Json<JsonValue>,
) -> Response {
    let (skill_name, tool_name) = skill_and_tool.0;

    Response::ok().stream(async move |tx| {
        let Some(agent) = Manager::get_by_skill(&skill_name).await else {
            let err_msg = format!("Agent for skill `{skill_name}` not found.");
            error!("[handle_tool_call] {err_msg}");
            let _ = tx.send(Event::Error(err_msg));
            return;
        };

        let agent_guard = agent.read().await;
        let sock_path = agent_guard.metadata.sock_path.to_string_lossy().to_string();
        let agent_name = agent_guard.metadata.name.clone();
        drop(agent_guard);

        let tool_call = ToolCall {
            id: "".into(),
            kind: "".into(),
            func: ToolCallFunction {
                name: tool_name,
                json_str: payload.0.to_string(),
            },
        };

        let client = Client::ipc(&sock_path);

        match handle_tool(
            client,
            sock_path,
            agent_name,
            skill_name,
            tool_call,
            tx.clone(),
        )
        .await
        {
            Ok((_, result_text)) => {
                let _ = tx.send(Event::Answer(result_text));
                let _ = tx.send(Event::Finish);
            }
            Err(e) => {
                error!("[handle_tool_call] Execution failed: {e}");
                let _ = tx.send(Event::Error(e.to_string()));
            }
        }
    })
}

/// Performs a single call to the agent's tool via IPC.
#[log()]
pub async fn handle_tool(
    client: Client,
    sock_path: String,
    agent_name: String,
    skill_name: String,
    tool_call: ToolCall,
    tx: Sender<Bytes>,
) -> Result<(String, String)> {
    let func = tool_call.func;
    let tool_call_id = tool_call.id;
    let log_json = func.json_str.replace('\n', " ");

    let msg = format!(
        "Calling `{agent_name}.{skill_name}.{}` tool: {log_json}",
        func.name
    );
    info!("{msg}");
    tx.send(Event::Thinking(msg)).ok();

    let request_path = format!("/skills/{skill_name}/call/{}", func.name);
    let request_body = func.parse_args::<JsonValue>()?;
    let mut response = client
        .post(&request_path)
        .header("Content-Type", "application/json")
        .json(&request_body)
        .stream::<Event>()
        .await;

    // error checking and auto-recovery of the agent if necessary
    if let Err(e) = &response {
        warn!("Agent `{agent_name}` didn't respond for tool call: {e}");
        tx.send(Event::Thinking(format!(
            "Agent `{agent_name}` didn't respond for tool call."
        )))?;

        if let Some(agent) = Manager::get_agent(&agent_name).await {
            if agent.read().await.ensure().await.is_ok() {
                response = Client::ipc(&sock_path)
                    .post(&request_path)
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .stream::<Event>()
                    .await;
            }
        } else {
            error!("Agent `{agent_name}` is unavailable for now.");
            return Err(
                Error::Custom(format!("Agent `{agent_name}` is unavailable for now.")).into(),
            );
        }
    }

    let mut stream = match response {
        Ok(res) => res,
        Err(e) => {
            return Err(Error::Custom(format!(
                "Agent `{agent_name}` crashed and failed to recover: {e}"
            ))
            .into());
        }
    };

    let mut full_text = str!();

    while let Some(event) = atoman::select! {
        _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
        res = stream.recv() => res?,
    } {
        match event {
            Event::Answer(text) => full_text.push_str(&text),
            Event::Finish => {}
            Event::Dialog(d_event) => {
                let d_event_id = d_event.get_id().to_string();
                let mut callback = Callback::register(&d_event_id).await;
                tx.send(Event::Dialog(d_event))?;

                let response = atoman::select! {
                    _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
                    res = callback.recv::<JsonValue>(Duration::from_secs(120)) => res?,
                };

                if let Some(value) = response {
                    let client = Client::ipc(&sock_path);
                    let _ = client
                        .post(&format!("/callback/{d_event_id}"))
                        .header("Content-Type", "application/json")
                        .json(&value)
                        .send()
                        .await
                        .map_err(|e| {
                            Error::Custom(format!(
                                "Failed to send callback to `{agent_name}` agent: {e}"
                            ))
                        })?;
                }
            }
            event => {
                let _ = tx.send(event);
            }
        }
    }

    Ok((tool_call_id, full_text))
}
