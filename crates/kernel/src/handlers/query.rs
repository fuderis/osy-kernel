use crate::{context, manager::*, prelude::*, runtime::Runtime, session::Session, skills};

use anylm::{
    api::{Content, Message, Messages},
    completions::{Chunk, Completions},
    embeddings::EmbeddingSearch,
};
use chrono::FixedOffset;
use ovsy_share::{Event, EventKind, HandleQuery, SessionInfo};
use std::collections::HashSet;
use tokio::task::JoinSet;

/// API: The user query handler
pub async fn handle_user_query(Paths(sid): Paths<SessionId>, data: Json<HandleQuery>) -> Response {
    let HandleQuery { message } = data.0;

    Response::ok().stream(move |tx| async move {
        let result = match read_session(sid).await {
            Ok((session, messages)) => {
                handle_query(sid, tx.clone(), session, messages, message).await
            }
            Err(e) => Err(e),
        };

        if let Err(e) = result {
            error!("[handle_query{{sid={sid}}}] {e}");
            tx.send(Event::error(str!(e))).ok();
        }
    })
}

/// Helper method to read user session from database
#[log(skip_all, fields(sid = %sid))]
async fn read_session(sid: SessionId) -> Result<(Arc<Mutex<Session>>, Arc<Mutex<Messages>>)> {
    info!("Reading the user session...");

    let Some(session) = Session::get(&sid) else {
        return Err(Error::UnknownSessionId(sid).into());
    };
    let db_messages = session.lock().await.read_messages().await?;
    let messages = arc_mutex!(Messages::from(db_messages));

    Ok((session, messages))
}

/// Handles the user query with self-healing on planning/generation level
#[log(skip_all, fields(sid = %sid))]
async fn handle_query(
    sid: SessionId,
    tx: Sender<Bytes>,
    session: Arc<Mutex<Session>>,
    messages: Arc<Mutex<Messages>>,
    message: Message,
) -> Result<()> {
    info!("Processing the user query...");

    let settings = Settings::get();
    let completions_options = settings.completions.options.clone();
    let exec_options = &settings.execution;
    let context_options = &settings.context;

    // 1. RAG: Search for relevant facts about the user
    let session_guard = session.lock().await;
    let mut facts_prompt = String::new();

    if let Some(user_text) = context::extract_text_from_msg(&message) {
        if let Ok(query_vec) = context::generate_embedding(&user_text, EmbeddingSearch::Query).await
        {
            if let Ok(facts) = session_guard
                .search_facts(
                    query_vec,
                    context_options.search_limit,
                    context_options.fact_similarity,
                )
                .await
            {
                if !facts.is_empty() {
                    info!(
                        "Loaded {} facts for sid={}: {:?}",
                        facts.len(),
                        sid,
                        facts.iter().map(|f| &f.data.text).collect::<Vec<_>>()
                    );

                    facts_prompt.push_str("\n\n### Loaded User Facts (use them when writing the answer, if necessary):\n");
                    for record in facts {
                        facts_prompt
                            .push_str(&format!("  * [ID: {}] {}\n", record.id, record.data.text));
                    }
                } else {
                    info!("No relevant facts found for user query.");
                }
            }
        }
    }

    // 2. Preparing the context and system promptes
    let raw_messages = messages.lock().await.messages.clone();
    let base_system_prompt = system_prompt(&session_guard.info, &settings);
    drop(session_guard);

    let messages = Messages::from(raw_messages)
        .system(vec![
            format!("{base_system_prompt}{facts_prompt}").into(),
            settings
                .completions
                .assist_prompt
                .replace("{AGENTS_LIST}", &Manager::agents_list_doc().await)
                .into(),
        ])
        .message(message)
        .wrap();

    let mut tasks_list = vec![];
    let mut evals_list = vec![];
    let mut memory_results = vec![];

    let mut retry_count = 0;
    let max_retries = exec_options.max_retries.max(1);

    // top-level generation cycle: task planning
    loop {
        tasks_list.clear();
        evals_list.clear();
        memory_results.clear();
        let mut text_response = str!();

        let mut response = match Completions::try_from(completions_options.clone())?
            .tools(Manager::basic_tools().await)
            .send(messages.clone())
            .await
        {
            Ok(res) => res,
            Err(e) => {
                retry_count += 1;
                if retry_count < max_retries {
                    warn!(
                        "Failed to send query completions request (attempt {retry_count}/{max_retries}): {e}"
                    );
                    messages.lock().await.add_user(vec![
                        format!("An error occurred: {e}. Please try again to plan the task using the tools.").into()
                    ]);
                    continue;
                } else {
                    return Err(e.into());
                }
            }
        };

        // read ai chunks and collect tool calls
        let mut chunk_error = None;
        while let Some(chunk) = response.next().await {
            match chunk {
                Ok(Chunk::Text(text_part)) => {
                    text_response.push_str(&text_part);
                    // streaming plain text to the user
                    tx.send(Event::answer(text_part))?;
                }

                Ok(Chunk::Tool(tool_call)) => match tool_call.func.name.as_ref() {
                    "handle_agent" => match tool_call.parse_args::<skills::task::TaskAction>() {
                        Ok(mut task) => {
                            task.tool_call_id = tool_call.id;
                            tasks_list.push(task);
                        }
                        Err(e) => {
                            chunk_error = Some(str!("Failed to parse handle_agent: {e}").into());
                            break;
                        }
                    },

                    "javascript_eval" => match tool_call.parse_args::<skills::eval::EvalAction>() {
                        Ok(eval) => {
                            evals_list.push((tool_call.id, eval));
                        }
                        Err(e) => {
                            chunk_error = Some(str!("Failed to parse javascript_eval: {e}").into());
                            break;
                        }
                    },

                    "remember_fact" => match tool_call
                        .parse_args::<skills::fact::RememberFactAction>()
                    {
                        Ok(act) => {
                            let s = session.lock().await;
                            match context::fact::handle_remember(&s, act.fact).await {
                                Ok(res_msg) => memory_results.push((tool_call.id, res_msg)),
                                Err(e) => {
                                    chunk_error = Some(str!("Failed to save fact: {e}").into());
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            chunk_error = Some(str!("Failed to parse remember_fact: {e}").into());
                            break;
                        }
                    },

                    "forget_fact" => match tool_call.parse_args::<skills::fact::ForgetFactAction>()
                    {
                        Ok(act) => {
                            let s = session.lock().await;
                            match context::fact::handle_forget(&s, act.fact_id).await {
                                Ok(res_msg) => memory_results.push((tool_call.id, res_msg)),
                                Err(e) => {
                                    chunk_error = Some(str!("Failed to remove fact: {e}").into());
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            chunk_error = Some(str!("Failed to parse forget_fact: {e}").into());
                            break;
                        }
                    },
                    _ => {}
                },

                Err(e) => {
                    chunk_error = Some(e.into());
                    break;
                }
            }
        }

        if let Some(err) = chunk_error {
            retry_count += 1;
            if retry_count < max_retries {
                warn!("Stream error on planning level ({retry_count}/{max_retries}): {err}");
                messages.lock().await.add_user(vec![
                    format!("An error occurred during stream generation: {err}. Please try again to complete the request.").into()
                ]);
                continue;
            } else {
                return Err(err);
            }
        }

        // hallucination check (if there is no text, no tasks, no JS calculations, and no memory operations)
        if tasks_list.is_empty()
            && evals_list.is_empty()
            && memory_results.is_empty()
            && text_response.trim().is_empty()
        {
            retry_count += 1;
            if retry_count < max_retries {
                warn!(
                    "Model hallucinated: empty text response and no tool calls. Retrying ({retry_count}/{max_retries})..."
                );
                messages.lock().await.add_user(vec![
                    "You returned an empty response. If you need to solve the task, delegate work to an agent, execute JS, or use memory tools.".into()
                ]);
                continue;
            } else {
                return Err(str!("Model failed to plan tasks: returned empty response").into());
            }
        }

        break;
    }

    // if the model has performed memory operations, notify the user
    for (tool_call_id, res_text) in memory_results {
        tx.send(Event::think(format!("{res_text}")).raw_task_info(0, tool_call_id))?;
    }

    // performing JS calculations (if any)
    if !evals_list.is_empty() {
        let mut runtime = Runtime::new();

        for (tool_call_id, eval) in evals_list {
            let result: String = runtime.eval(&eval.code)?;

            if let Some(task_id) = eval.task_id {
                let Some(task) = tasks_list.iter_mut().find(|t| t.task_id == task_id) else {
                    warn!("Task #{task_id} not found");
                    continue;
                };

                if let Some(parameter) = &eval.parameter {
                    let placeholder = format!("{{{{{parameter}}}}}");

                    if task.task_query.contains(&placeholder) {
                        task.task_query = task.task_query.replace(&placeholder, &result);
                    } else {
                        warn!("Placeholder '{parameter}' not found in task #{task_id}");
                    }
                } else {
                    if !task.task_query.ends_with('\n') {
                        task.task_query.push('\n');
                    }

                    task.task_query.push_str(&result);
                }
            } else {
                tx.send(Event::answer(format!("\n\n{result}")).raw_task_info(0, tool_call_id))?;
            }
        }
    }

    // launching an Agent task pool
    if !tasks_list.is_empty() {
        // remove broken dependencies:
        let active_ids: HashSet<i64> = tasks_list.iter().map(|task| task.task_id).collect();
        for task in tasks_list.iter_mut() {
            task.depend_tasks.retain(|id| active_ids.contains(id));
        }

        // send tool calls to client:
        if let Some(msg) = (&*messages.lock().await).messages.last()
            && msg.role.is_assistant()
        {
            tx.send(Event::start(&msg.tool_calls))?;
        }

        // delegate tasks:
        let tasks_len = tasks_list.len();
        let tasks = Tasks::new(session, messages);

        // collect tasks:
        let mut running = vec![];
        {
            let mut lock = tasks.lock().await;

            for task in tasks_list {
                if task.depend_tasks.is_empty() {
                    running.push(task.task_id);
                }

                lock.pending
                    .insert(task.task_id, Task::new(tx.clone(), tasks.clone(), task));
            }
        };

        // spawning tasks:
        info!("Spawning agent tasks ({tasks_len})");
        for task_id in running {
            handle_task(task_id, tx.clone(), tasks.clone()).await;
        }
    } else {
        tx.send(Event::finish())?;
        info!("The user request was processed without agent tasks");

        // save messages to database:
        let to_save = messages.lock().await.slice(-1);
        session.lock().await.write_messages(to_save).await?;
    }

    Ok(())
}

/// Handles the agent task or pendings it
#[async_recursion]
#[log(skip_all, fields(tid))]
pub async fn handle_task(tid: i64, tx: Sender<Bytes>, tasks: Arc<Mutex<Tasks>>) {
    let mut lock = tasks.lock().await;
    let Some(task) = lock.pending.remove(&tid) else {
        return;
    };

    let tx = tx.clone();
    let tasks = tasks.clone();

    // handle agent task:
    let messages = lock.messages.clone();
    let current = Span::current();
    let child = tokio::spawn(
        async move {
            let session = tasks.lock().await.session.clone();

            if let Err(e) = handle_agent(
                task.agent.clone(),
                session,
                messages,
                tx.clone(),
                task.clone(),
            )
            .await
            {
                error!("{e}");
                // send error to client
                task.tx
                    .send(Event::error(str!("{e}")).task_info(task.info()))
                    .ok();

                // guarantee that client will receive the task closure
                task.tx.send(Event::finish().task_info(task.info())).ok();

                task.finish_branch().await;
            }
        }
        .instrument(current),
    );

    lock.working.insert(tid, arc!(child));
}

/// Handles the agent task
#[log(skip_all, fields(agent = %agent_name, skills = %task.skills.join(",")))]
pub async fn handle_agent(
    agent_name: String,
    session: Arc<Mutex<Session>>,
    messages: Arc<Mutex<Messages>>,
    tx: Sender<Bytes>,
    task: Task,
) -> Result<()> {
    let arc_name = arc!(task.agent.clone());

    // 1. Checking the agent for existence
    let (sock_path, prompt, _skills) = match Manager::ensure_agent(&arc_name).await {
        Ok(Some(ops)) => ops,
        _ => {
            return Err(str!("Agent `{}` is not available or failed to start", task.agent).into());
        }
    };

    // 2. Getting tools via IPC
    let client = Client::ipc(&sock_path.to_string_lossy());
    let response = client
        .post("/tools/list")
        .header("Content-Type", "application/json")
        .json(&json!({ "skills": task.skills }))
        .send()
        .await?;
    let tools = response.json::<Vec<anylm::api::Tool>>().await?;

    // warn!("Received Tools List: {tools:#?}"); // DEBUG

    // logging and sending the start event
    let log_query = task
        .query
        .chars()
        .take(40)
        .collect::<String>()
        .trim_end_matches('.')
        .replace('\n', "\\n");

    info!("Handling `{}` agent: \"{log_query}...\"", task.agent);
    tx.send(
        Event::think(str!(
            "**Handling `{}` agent:** *\"{log_query}...\"*",
            task.agent
        ))
        .task_info(task.info()),
    )
    .ok();

    let settings = Settings::get();
    let options = settings.completions.options.clone();
    let exec_options = &settings.execution;

    // 3. Creating a local context for generating
    let system_pr = system_prompt(&session.lock().await.info, &settings);

    // collect the context in a text block for the system prompt
    let context_items = task
        .context()
        .await
        .into_iter()
        .flatten()
        .collect::<Vec<_>>();
    let context_str = if !context_items.is_empty() {
        str!("Prior Context / Task Results:\n{:?}", context_items)
    } else {
        String::new()
    };

    let agent_messages = Messages::new()
        .system(vec![
            format!("{}{}", system_pr, context_str).into(),
            prompt.trim().into(),
        ])
        .user(vec![
            str!("{prompt}\n\n{query}",
                prompt = "For the following request, you MUST use the provided tools and MUST NOT answer from your own knowledge. If no suitable tool is available, return an error explaining that the required tool does not exist. Never invent or assume tools that were not provided.",
                query = task.query
            ).into()
        ])
        .wrap();

    // warn!("{agent_messages:#?}"); // DEBUG

    let mut tool_calls = vec![];
    let mut retry_count = 0;
    let max_retries = exec_options.max_retries.max(1);

    // the self-healing cycle
    loop {
        tool_calls = vec![];
        let mut text_response = str!();

        let response_res = Completions::try_from(options.clone())?
            .tools(tools.clone())
            .send(agent_messages.clone())
            .await;

        match response_res {
            Ok(mut response) => {
                let mut chunk_error = None;
                while let Some(chunk) = response.next().await {
                    match chunk {
                        Ok(Chunk::Text(text_part)) => {
                            text_response.push_str(&text_part);
                            tx.send(Event::answer(text_part).task_info(task.info()))?;
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

                // self-healing in case of a stream error
                if let Some(err) = chunk_error {
                    retry_count += 1;
                    if retry_count < max_retries {
                        warn!(
                            "Error reading stream from agent `{}`. Retrying ({retry_count}/{max_retries}): {err}",
                            task.agent
                        );
                        tx.send(
                            Event::think(str!(
                                "Stream error. Healing and retrying `{}` agent execution...",
                                task.agent
                            ))
                            .task_info(task.info()),
                        )
                        .ok();
                        agent_messages.lock().await.add_user(vec![
                            format!("An error occurred during output generation: {err}. Please try again and complete the task using the available tools.").into()
                        ]);
                        continue;
                    } else {
                        return Err(str!(
                            "Agent `{}` failed after stream error: {err}",
                            task.agent
                        )
                        .into());
                    }
                }

                // self-healing with an empty response without calling tools
                if tool_calls.is_empty() && text_response.trim().is_empty() {
                    retry_count += 1;
                    if retry_count < max_retries {
                        warn!(
                            "Agent `{}` returned empty response and no tool calls. Retrying ({retry_count}/{max_retries})...",
                            task.agent
                        );
                        tx.send(Event::think(str!("Agent `{}` returned empty response. Self-healing task execution...", task.agent)).task_info(task.info())).ok();
                        agent_messages.lock().await.add_user(vec![
                            "You did not call any tools. Please execute the requested task using the available tools now.".into()
                        ]);
                        continue;
                    } else {
                        return Err(str!(
                            "Agent `{}` failed to execute task after {} retries: empty output",
                            task.agent,
                            max_retries
                        )
                        .into());
                    }
                }
            }

            Err(e) => {
                retry_count += 1;
                if retry_count < max_retries {
                    warn!(
                        "Failed to send request to Completions for agent `{}`. Retrying ({retry_count}/{max_retries}): {e}",
                        task.agent
                    );
                    tx.send(
                        Event::think(str!(
                            "Request error. Healing and retrying `{}` agent execution...",
                            task.agent
                        ))
                        .task_info(task.info()),
                    )
                    .ok();
                    agent_messages.lock().await.add_user(vec![
                        format!("Failed to process request due to error: {e}. Please attempt to execute the task again using tools.").into()
                    ]);
                    continue;
                } else {
                    return Err(str!(
                        "Agent `{}` failed sending completions request: {e}",
                        task.agent
                    )
                    .into());
                }
            }
        }

        // if there are no tools to call - exit the generation cycle
        if tool_calls.is_empty() {
            break;
        }

        // parallel execution of tool calls
        let mut workers = JoinSet::new();

        for tool_call in tool_calls {
            let client = client.clone();
            let sock_path = sock_path.clone();
            let arc_name = arc_name.clone();
            let tx = tx.clone();
            let task = task.clone();

            workers.spawn(async move {
                let func = tool_call.func;
                let log_json = func.json_str.replace('\n', "\\n");

                info!("Calling `{} -> {}` tool: {log_json}", task.agent, func.name);
                tx.send(
                    Event::think(str!(
                        "Calling `{} -> {}` tool: {log_json}",
                        task.agent,
                        func.name
                    ))
                    .task_info(task.info()),
                )
                .ok();

                let request_path = format!("/tools/call/{}", func.name);
                let request_body = func.parse_args::<JsonValue>()?;

                // sending a request to the agent's server
                let mut response = client
                    .post(&request_path)
                    .header("Content-Type", "application/json")
                    .json(&request_body)
                    .stream::<Event>()
                    .await;

                // tactical restart
                if response.is_err() {
                    warn!(
                        "Agent `{}` didn't respond. Attempting tactical restart...",
                        task.agent
                    );
                    tx.send(
                        Event::think(str!(
                            "Connection lost. Restarting `{}` agent...",
                            task.agent
                        ))
                        .task_info(task.info()),
                    )
                    .ok();

                    let _ = Manager::stop(arc_name.clone()).await;

                    if let Ok(Some((_, _, _))) = Manager::ensure_agent(&arc_name).await {
                        response = Client::ipc(&sock_path.to_string_lossy())
                            .post(&request_path)
                            .header("Content-Type", "application/json")
                            .json(&request_body)
                            .stream::<Event>()
                            .await;
                    }
                }

                let mut stream = match response {
                    Ok(res) => res,
                    Err(e) => {
                        return Err(str!(
                            "Agent `{}` crashed and failed to recover: {e}",
                            task.agent
                        )
                        .into());
                    }
                };

                let mut full_text = str!();

                while let Some(event) = stream.recv().await? {
                    match event.kind {
                        EventKind::Answer => {
                            full_text.push_str(&event.text);
                            tx.send(Event::answer(event.text).task_info(task.info()))?;
                        }
                        EventKind::Finish => {}
                        _ => {
                            tx.send(event.task_info(task.info()))?;
                        }
                    }
                }

                Ok::<String, DynError>(full_text)
            });
        }

        // collecting the results as they are completed and instantly recording them in the history
        while let Some(worker_result) = workers.join_next().await {
            let full_text: String =
                worker_result.map_err(|e| str!("Worker task panicked: {e}"))??;
            let content_item: Content = full_text.into();

            // write pointwise to the local context to continue generation in the loop
            agent_messages
                .lock()
                .await
                .push_content(None, content_item.clone());

            // save the intermediate stage to the global message history of the main chat
            messages
                .lock()
                .await
                .push_content(Some(&task.tool_call_id), content_item);
        }

        // cycle has completed without errors, stopping it
        break;
    }

    // take the accumulated results
    let agent_contents = {
        let mut local_lock = agent_messages.lock().await;
        std::mem::take(&mut local_lock.messages)
            .into_iter()
            .filter(|msg| !msg.role.is_assistant() && !msg.role.is_user())
            .flat_map(|msg| msg.content)
            .collect::<Vec<Content>>()
    };

    // completing the task in the client and pool
    tx.send(Event::finish().task_info(task.info())).ok();
    task.finish(agent_contents).await;

    // send control query (self-correction loop)
    if task.is_last().await {
        info!("All parallel tasks completed. Launching control query...");

        let control_msg = Message::user(vec![settings.completions.control_prompt.as_str().into()]);
        let sid = session.lock().await.id;

        if let Err(e) = handle_query(
            sid,
            tx.clone(),
            session.clone(),
            messages.clone(),
            control_msg,
        )
        .await
        {
            error!("[verification_loop{{sid={sid}}}] Failed to restart query loop: {e}");
            tx.send(Event::error(str!(e))).ok();
        }
    }

    Ok(())
}

/// Generates the system prompt
fn system_prompt(info: &SessionInfo, settings: &Settings) -> String {
    let now_utc = Utc::now();
    let now_local = now_local(info.timezone);

    settings
        .completions
        .system_prompt
        .trim()
        .replace(
            "{DATETIME_LOCAL}",
            &now_local
                .format("%A, %B %d, %Y, %I:%M:%S %p %Z")
                .to_string(),
        )
        .replace(
            "{DATETIME_GLOBAL}",
            &now_utc.format("%A, %B %d, %Y, %I:%M:%S %p UTC").to_string(),
        )
        .replace(
            "{CURRENT_PATH}",
            &info
                .current_path
                .clone()
                .map(|path| path.to_string_lossy().to_string())
                .unwrap_or_default(),
        )
}

/// Returns the session local date time
fn now_local(timezone_m: i16) -> DateTime<FixedOffset> {
    let offset_seconds = (timezone_m as i32) * 60;
    let tz =
        FixedOffset::east_opt(offset_seconds).unwrap_or_else(|| FixedOffset::east_opt(0).unwrap());

    let utc_now = Utc::now();
    utc_now.with_timezone(&tz)
}
