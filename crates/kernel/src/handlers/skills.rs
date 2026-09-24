use crate::{Manager, prelude::*, utils};

use anylm::{
    api::{Content, Message, Messages, Tool, ToolCall, ToolCallFunction},
    completions::{Chunk, Completions},
};
use atoman::task::JoinSet;
use osy_share::{Event, HandleQuery, SessionInfo};
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

/// API: Handles skill query.
pub async fn handle_skill_query(skill: Paths<String>, payload: Json<HandleQuery>) -> Response {
    let HandleQuery { message, info } = payload.0;
    let skill_name = skill.0;

    Response::ok().stream(async move |tx| {
        match handle_skill(tx.clone(), info.unwrap_or_default(), &skill_name, message).await {
            Ok(result_text) => {
                let _ = tx.send(Event::Answer(result_text));
                let _ = tx.send(Event::Finish);
            }
            Err(e) => {
                error!("[handle_skill_query] Execution failed: {e}");
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

/// Performs agent's skill (creates the context, launches the LLM cycle, self-healing and distribution of sub-tools).
#[log()]
pub async fn handle_skill(
    tx: Sender<Bytes>,
    session_info: SessionInfo,
    skill_name: &str,
    message: Message,
) -> Result<String> {
    let Some(agent) = Manager::get_by_skill(skill_name).await else {
        return Err(
            Error::Custom(format!("Using unknown skill `{skill_name}`, aborting...")).into(),
        );
    };

    // extracting agent metadata
    let agent_guard = agent.read().await;
    let sock_path = agent_guard.metadata.sock_path.to_string_lossy().to_string();
    let agent_name = agent_guard.metadata.name.clone();
    let Some(skill) = agent_guard.metadata.skills.get(skill_name) else {
        return Err(Error::Custom(format!(
            "Failed to get `{skill_name}` skill metadata, aborting..."
        ))
        .into());
    };
    let skill_prompt = skill.prompt.trim().to_owned();
    drop(agent_guard);

    // loading agent's list of tools
    let client = Client::ipc(&sock_path);
    let tools = client
        .post(&format!("/skills/{skill_name}/tools"))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            Error::Custom(format!(
                "Failed to get the `{agent_name}` agent tools list: {e}"
            ))
        })?
        .json::<Vec<Tool>>()
        .await?;

    let msg = format!(
        "Handling `{agent_name}.{skill_name}` skill: \"{}...\"",
        message.extract_texts()[0]
            .chars()
            .take(40)
            .collect::<String>()
            .trim_end_matches('.')
            .replace('\n', " ")
    );
    info!("{msg}");
    tx.send(Event::Thinking(msg)).ok();

    let cfg = Config::get();
    let provider_options = cfg
        .completions
        .options
        .clone()
        .temperature(cfg.completions.skill_temp);
    let exec_options = &cfg.execution;

    // assembling context messages
    let agent_messages = {
        let mut msgs = Messages::new();
        let mut system_content = vec![utils::system_prompt(&session_info, &cfg).into()];

        if !skill_prompt.is_empty() {
            system_content.push(skill_prompt.into());
        }

        msgs.add_system(system_content);
        msgs.add_system(vec![
            "For the following request, you MUST use the provided tools and MUST NOT answer from your own knowledge. \
             If no suitable tool is available, return an error explaining that the required tool does not exist. \
             Never invent or assume tools that were not provided.".into()
        ]);
        msgs.add_message(message);

        msgs.wrap()
    };

    let mut retry_count = 0;
    let max_retries = exec_options.max_retries.max(1);

    let mut iteration = 0;
    let max_iterations = exec_options.max_iterations.max(1);

    // cycle of generation and self-healing
    loop {
        if tx.is_closed() {
            return Err(Error::ConnectionClosed.into());
        }

        iteration += 1;
        if iteration > max_iterations {
            warn!(
                "Agent `{agent_name}` reached maximum allowed iterations ({max_iterations}) for skill `{skill_name}`."
            );
            return Err(Error::Custom(format!(
                "Agent `{agent_name}` exceeded maximum execution limit of {max_iterations} iterations."
            ))
            .into());
        }

        let mut tool_calls = vec![];
        let mut text_response = str!();

        match Completions::try_from(provider_options.clone())?
            .tools(tools.clone())
            .send(agent_messages.clone())
            .await
        {
            Ok(mut stream) => {
                let mut chunk_error = None;

                while let Some(chunk) = stream.next().await {
                    match chunk {
                        Ok(Chunk::Text(text_part)) => {
                            text_response.push_str(&text_part);
                        }
                        Ok(Chunk::Tool(tool_call)) => {
                            tool_calls.push(tool_call);
                        }
                        Err(e) => {
                            chunk_error = Some(e);
                            break;
                        }
                    }
                }

                if let Some(err) = chunk_error {
                    retry_count += 1;
                    if retry_count < max_retries {
                        warn!(
                            "Error reading stream from agent `{agent_name}`. Retrying ({retry_count}/{max_retries}): {err}"
                        );
                        tx.send(Event::Thinking(format!(
                            "Stream error. Retrying {agent_name} agent execution..."
                        )))?;

                        agent_messages.lock().await.add_user(vec![
                            format!("An error occurred during output generation: {err}. Please try again using tools.").into()
                        ]);

                        continue;
                    } else {
                        return Err(Error::Custom(format!(
                            "Agent `{agent_name}` failed after stream error: {err}"
                        ))
                        .into());
                    }
                }

                if tool_calls.is_empty() && text_response.trim().is_empty() {
                    retry_count += 1;
                    if retry_count < max_retries {
                        warn!(
                            "Agent `{agent_name}` returned empty response and no tool calls. Retrying ({retry_count}/{max_retries})..."
                        );
                        tx.send(Event::Thinking(format!(
                            "Agent {agent_name} returned empty response. Retrying task..."
                        )))?;

                        agent_messages.lock().await.add_user(vec![
                            "You did not call any tools. Please execute the requested task using tools now.".into()
                        ]);

                        continue;
                    } else {
                        return Err(Error::Custom(format!(
                            "Agent `{agent_name}` failed after {max_retries} retries: empty output"
                        ))
                        .into());
                    }
                }
            }
            Err(e) => {
                retry_count += 1;
                if retry_count < max_retries {
                    warn!(
                        "Failed to send completions request for agent `{agent_name}` ({retry_count}/{max_retries}): {e}"
                    );
                    tx.send(Event::Thinking(format!(
                        "Request error. Retrying {agent_name} agent execution..."
                    )))?;

                    agent_messages.lock().await.add_user(vec![
                        format!("Failed to process request due to error: {e}. Please attempt to execute the task again.").into()
                    ]);
                    continue;
                } else {
                    return Err(Error::Custom(format!(
                        "Agent `{agent_name}` failed sending completions request: {e}"
                    ))
                    .into());
                }
            }
        }

        if tx.is_closed() {
            return Err(Error::ConnectionClosed.into());
        }

        // if no tool calls, return the final response of the skill
        if tool_calls.is_empty() {
            return Ok(format!("Agent `{agent_name}` response:\n{text_response}"));
        }

        let mut sub_workers = JoinSet::new();

        // parallel execution of sub-tool calls via handle_tool
        for tool_call in tool_calls {
            let client = client.clone();
            let sock_path = sock_path.clone();
            let agent_name = agent_name.clone();
            let tx = tx.clone();
            let skill_name = skill_name.to_string();

            sub_workers.spawn(
                async move {
                    handle_tool(client, sock_path, agent_name, skill_name, tool_call, tx).await
                }
                .log_span(Span::current()),
            );
        }

        // collecting results of work of the sub-tools
        loop {
            atoman::select! {
                _ = tx.closed() => {
                    sub_workers.abort_all();
                    return Err(Error::ConnectionClosed.into());
                }
                maybe_res = sub_workers.join_next() => {
                    match maybe_res {
                        Some(Ok(Ok((tool_call_id, result_text)))) => {
                            let mut guard = agent_messages.lock().await;
                            guard.push_content(
                                Some(&tool_call_id),
                                Content::text(format!("Tool execution result:\n{result_text}")),
                            );
                        }
                        Some(Ok(Err(e))) => {
                            error!("Sub-tool execution failed: {e}");
                            return Err(e);
                        }
                        Some(Err(e)) => {
                            error!("Sub-tool worker panicked: {e}");
                            return Err(e.into());
                        }
                        None => break,
                    }
                }
            }
        }
    }
}
