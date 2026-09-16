use crate::{Manager, prelude::*, user::Session, utils};

use anylm::{
    api::{Content, Messages, Schema, Tool},
    completions::{Chunk, Completions},
};
use osy_share::Event;
use pearce::Callback;
use serde::{Deserialize, Serialize};
use tokio::task::JoinSet;

/// Returns tools list.
pub fn tools_list() -> Vec<Tool> {
    vec![
        Tool::new("handle_task", "Delegates a task using a specific skill.")
            .required_property(
                "skill",
                Schema::string("The existing skill required for this task."),
            )
            .required_property(
                "query",
                Schema::string("The detailed task prompt, parameters, and input context."),
            ),
    ]
}

/// Agent task data.
#[derive(Default, Debug, Clone, Serialize, Deserialize)]
pub struct TaskAction {
    #[serde(default)]
    pub tool_call_id: String,

    /// Target skill name.
    pub skill: String,
    /// Agent task query.
    pub query: String,
}

/// Handles agent task (isolated).
#[log(skip_all, fields(skill = %task.skill))]
pub async fn handle_task(
    tx: Sender<Bytes>,
    session: Arc<Mutex<Session>>,
    messages: Arc<Mutex<Messages>>,
    task: TaskAction,
) -> Result<()> {
    let skill_name = &task.skill;
    let Some(agent) = Manager::get_by_skill(skill_name).await else {
        return Err(
            Error::Custom(format!("Using unknown skill `{}`, aborting...", skill_name)).into(),
        );
    };

    // extract agent info
    let agent_guard = agent.read().await;
    let sock_path = agent_guard.metadata.sock_path.to_string_lossy().to_string();
    let agent_name = agent_guard.metadata.name.clone();
    let Some(skill) = agent_guard.metadata.skills.get(skill_name) else {
        return Err(Error::Custom(format!(
            "Failed to get `{}` skill metadata, aborting...",
            skill_name
        ))
        .into());
    };
    let skill_prompt = skill.prompt.trim().to_owned();
    drop(agent_guard);

    // receive agent tools from server
    let client = Client::ipc(&sock_path);
    let tools = client
        .post(&format!("/skills/{}/tools", task.skill))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| {
            Error::Custom(format!(
                "Failed to get the `{}` agent tools list: {e}",
                agent_name
            ))
        })?
        .json::<Vec<Tool>>()
        .await?;

    // logging the handling start
    let msg = format!(
        "Handling `{agent_name}.{skill_name}` skill: \"{}...\"",
        task.query
            .chars()
            .take(40)
            .collect::<String>()
            .trim_end_matches('.')
            .replace('\n', " ")
    );
    info!("{msg}");
    tx.send(Event::Thinking(msg)).ok();

    let settings = Settings::get();
    let options = settings.completions.options.clone();
    let exec_options = &settings.execution;

    // create local context [Messages]
    let agent_messages = {
        let mut msgs = Messages::new();

        // add system prompt
        let mut system_content =
            vec![utils::system_prompt(&session.lock().await.info, &settings).into()];

        if !skill_prompt.is_empty() {
            system_content.push(skill_prompt.into());
        }

        msgs.add_system(system_content);

        // add user prompt
        msgs.add_user(vec![
            format!(
                "{prompt}\n\n{query}",
                prompt = "For the following request, you MUST use the provided tools and MUST NOT answer from your own knowledge. \
                  If no suitable tool is available, return an error explaining that the required tool does not exist. \
                  Never invent or assume tools that were not provided.",
                query = task.query
            ).into()
        ]);

        msgs.wrap()
    };

    let mut retry_count = 0;
    let max_retries = exec_options.max_retries.max(1);

    // self-healing execution loop
    loop {
        if tx.is_closed() {
            return Err(Error::ConnectionClosed.into());
        }

        let mut tool_calls = vec![];
        let mut text_response = str!();

        // send request to LLM
        match Completions::try_from(options.clone())?
            .tools(tools.clone())
            .send(agent_messages.clone())
            .await
        {
            Ok(mut stream) => {
                let mut chunk_error = None;

                // collect stream chunks
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

                // check for stream errors
                if let Some(err) = chunk_error {
                    retry_count += 1;

                    if retry_count < max_retries {
                        warn!(
                            "Error reading stream from agent `{agent_name}`. Retrying ({retry_count}/{max_retries}): {err}"
                        );
                        tx.send(Event::Thinking(format!(
                            "Stream error. Retrying {} agent execution...",
                            agent_name
                        )))?;

                        // add correction message
                        agent_messages.lock().await.add_user(vec![
                            format!("An error occurred during output generation: {err}. Please try again using tools.").into()
                        ]);

                        // start new healing cycle
                        continue;
                    } else {
                        // interrupt self-healing loop (recursion limit)
                        return Err(Error::Custom(format!(
                            "Agent `{agent_name}` failed after stream error: {err}"
                        ))
                        .into());
                    }
                }

                // check final result for empty
                if tool_calls.is_empty() && text_response.trim().is_empty() {
                    retry_count += 1;
                    if retry_count < max_retries {
                        warn!(
                            "Agent `{agent_name}` returned empty response and no tool calls. Retrying ({retry_count}/{max_retries})..."
                        );
                        tx.send(Event::Thinking(format!(
                            "Agent {} returned empty response. Retrying task...",
                            agent_name
                        )))?;

                        // add correction message
                        agent_messages.lock().await.add_user(vec![
                            "You did not call any tools. Please execute the requested task using tools now.".into()
                        ]);

                        // start new healing cycle
                        continue;
                    } else {
                        // interrupt self-healing loop (recursion limit)
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
                        "Request error. Retrying {} agent execution...",
                        agent_name
                    )))?;

                    // add correction message
                    agent_messages.lock().await.add_user(vec![
                        format!("Failed to process request due to error: {e}. Please attempt to execute the task again.").into()
                    ]);
                    continue;
                } else {
                    // interrupt self-healing loop (recursion limit)
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

        // if no sub‑tool calls, return final response
        if tool_calls.is_empty() {
            messages.lock().await.push_content(
                Some(&task.tool_call_id),
                Content::text(format!("Agent `{agent_name}` response:\n{text_response}")),
            );

            // stop self-healing loop (handling finished)
            break;
        }

        let mut sub_workers = JoinSet::new();

        // parallel execution of sub-tool calls via IPC
        for tool_call in tool_calls {
            let client = client.clone();
            let sock_path = sock_path.clone();
            let agent_name = agent_name.clone();
            let tx = tx.clone();
            let agent_name = agent_name.to_string();
            let skill_name = skill_name.to_string();

            // spawn tool worker
            sub_workers.spawn(
                async move {
                    let func = tool_call.func;
                    let tool_call_id = tool_call.id;
                    let log_json = func.json_str.replace('\n', " ");

                    // logging the tool call start
                    let msg = format!(
                        "Calling `{agent_name}.{skill_name}.{}` tool: {log_json}",
                        func.name
                    );
                    info!("{msg}");
                    tx.send(Event::Thinking(msg)).ok();

                    // send tool call request
                    let request_path = format!("/skills/{}/call/{}", skill_name, func.name);
                    let request_body = func.parse_args::<JsonValue>()?;
                    let mut response = client
                        .post(&request_path)
                        .header("Content-Type", "application/json")
                        .json(&request_body)
                        .stream::<Event>()
                        .await;

                    // check for errors
                    if let Err(e) = &response {
                        warn!("Agent `{agent_name}` didn't respond for tool call: {e}");
                        tx.send(Event::Thinking(format!(
                            "Agent `{agent_name}` didn't respond for tool call."
                        )))?;

                        // recover agent if necessary
                        if let Some(agent) = Manager::get_agent(&agent_name).await {
                            if let Ok(_) = agent.read().await.ensure().await {
                                response = Client::ipc(&sock_path)
                                    .post(&request_path)
                                    .header("Content-Type", "application/json")
                                    .json(&request_body)
                                    .stream::<Event>()
                                    .await;
                            }
                        } else {
                            // stop execution - agent is unavailable
                            error!("Agent `{agent_name}` is unavailable for now.");
                            return Err(Error::Custom(format!(
                                "Agent `{agent_name}` is unavailable for now."
                            ))
                            .into());
                        }
                    }

                    // check the tool response
                    let mut stream = match response {
                        Ok(res) => res,
                        Err(e) => {
                            // stop execution - agent crashed
                            return Err(Error::Custom(format!(
                                "Agent `{agent_name}` crashed and failed to recover: {e}"
                            ))
                            .into());
                        }
                    };

                    let mut full_text = str!();

                    // collect all events
                    while let Some(event) = tokio::select! {
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

                                let response = tokio::select! {
                                    _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
                                    res = callback.recv::<JsonValue>(Duration::from_secs(120)) => res?,
                                };

                                if let Some(value) = response {
                                    // send callback to agent server
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

                    Ok::<(String, String), DynError>((tool_call_id, full_text))
                }
                .instrument(Span::current()),
            );
        }

        // collect results of subtools
        loop {
            tokio::select! {
                _ = tx.closed() => {
                    sub_workers.abort_all();
                    return Err(Error::ConnectionClosed.into());
                }
                maybe_res = sub_workers.join_next() => {
                    match maybe_res {
                        Some(Ok(Ok((tool_call_id, result_text)))) => {
                            // push result into agent messages
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

    Ok(())
}
