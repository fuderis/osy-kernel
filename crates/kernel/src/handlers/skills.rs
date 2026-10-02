use crate::{
    Manager,
    prelude::*,
    skills::{self, EvalAction, RememberFact, SearchFact},
    user::{Session, UserState},
    utils,
};

use anylm::{
    api::{Content, Message, Messages, Tool},
    completions::{Chunk, Completions},
};
use atoman::task::JoinSet;
use osy_share::{Event, HandleQuery, SessionId, SessionInfo};

/// API: Handles skill query.
pub async fn handle_skill_query(
    paths: Paths<(SessionId, String)>,
    payload: Json<HandleQuery>,
) -> Response {
    let (sid, skill_name) = paths.0;
    warn!("HIT: sid={}, skill={}", sid, skill_name);

    let HandleQuery { message } = payload.0;

    Response::ok().stream(move |tx| async move {
        let result = match Session::read(sid).await {
            Ok((session, _messages)) => {
                if let Err(e) = session.lock().await.write_message(message.clone()).await {
                    error!("[handle_skill_query{{sid={sid}}}] Failed to write user message: {e}");
                    Err(e)
                } else {
                    let session_info = session.lock().await.info.clone();

                    match handle_skill(tx.clone(), session_info, &skill_name, message, true).await {
                        Ok(result_text) => {
                            let assistant_msg = Message::assistant(vec![result_text.clone().into()],vec![]);
                            if let Err(e) = session.lock().await.write_message(assistant_msg).await {
                                error!("[handle_skill_query{{sid={sid}}}] Failed to write assistant message: {e}");
                                Err(e)
                            } else {
                                let _ = tx.send(Event::Finish);
                                Ok(())    
                            }
                        }
                        Err(e) => Err(e),
                    }    
                }
            }
            Err(e) => Err(e),
        };

        if let Err(e) = result {
            error!("[handle_skill_query{{sid={sid}}}] Execution failed: {e}");
            tx.send(Event::Error(e.to_string())).ok();
        }
    })
}

/// Performs agent's skill (creates the context, launches the LLM cycle, self-healing and distribution of sub-tools).
#[log()]
pub async fn handle_skill(
    tx: Sender<Bytes>,
    session_info: SessionInfo,
    skill_name: &str,
    message: Message,
    stream_answer: bool
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
    let agent_skill_prompt = skill.prompt.trim().to_owned();
    drop(agent_guard);

    // loading agent's list of tools and combining with skill basic tools
    let client = Client::ipc(&sock_path);
    let mut tools = Manager::skill_basic_tools().await;
    let agent_tools = client
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
    tools.extend(agent_tools);

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

    // assembling context messages with config skill prompt & agent skill prompt
    let agent_messages = {
        let mut msgs = Messages::new();
        let mut system_content = vec![utils::system_prompt(&session_info, &cfg).into()];

        let config_skill_prompt = cfg.prompts.skill_prompt.trim();
        if !config_skill_prompt.is_empty() {
            system_content.push(config_skill_prompt.to_string().into());
        }

        if !agent_skill_prompt.is_empty() {
            system_content.push(agent_skill_prompt.into());
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
                            if stream_answer {
                                if let Err(e) = tx.send(Event::Answer(text_part.clone())) {
                                    warn!("[handle_skill] Failed to stream chunk to client: {e}");
                                }
                            }
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

        // parallel execution of sub-tool calls (handling both basic system tools and IPC tools)
        for tool_call in tool_calls {
            let client = client.clone();
            let sock_path = sock_path.clone();
            let agent_name = agent_name.clone();
            let tx = tx.clone();
            let skill_name = skill_name.to_string();
            let session_info = session_info.clone();

            sub_workers.spawn(
                async move {
                    match tool_call.func.name.as_str() {
                        "js_eval" => {
                            let eval: EvalAction = tool_call.parse_args()?;
                            tx.send(Event::Thinking(format!(
                                "Executing JavaScript code: {:80}...",
                                &eval.code
                            )))
                            .ok();
                            let result = skills::handle_eval(eval).await?;
                            Ok((tool_call.id, format!("JS Result:\n{result}")))
                        }
                        "remember_fact" => {
                            let act: RememberFact = tool_call.parse_args()?;
                            let user = UserState::get_or_init(session_info.id.user_id).await?;
                            let user_guard = user.read().await;
                            let res_msg = skills::handle_remember_fact(&user_guard, act).await?;
                            tx.send(Event::Thinking(res_msg.clone())).ok();
                            Ok((tool_call.id, format!("Memory Operation Result:\n{res_msg}")))
                        }
                        "search_fact" => {
                            let act: SearchFact = tool_call.parse_args()?;
                            let user = UserState::get_or_init(session_info.id.user_id).await?;
                            let user_guard = user.read().await;
                            let res_msg = skills::handle_search_fact(&user_guard, act).await?;
                            tx.send(Event::Thinking(res_msg.clone())).ok();
                            Ok((tool_call.id, format!("Memory Operation Result:\n{res_msg}")))
                        }
                        _ => {
                            super::handle_tool(client, sock_path, agent_name, skill_name, tool_call, tx)
                                .await
                        }
                    }
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
