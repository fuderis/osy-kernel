use crate::{
    context, helpers,
    manager::Manager,
    prelude::*,
    runtime::Runtime,
    session::Session,
    skills::{self, task::TaskAction},
};

use anylm::{
    api::{Content, Message, Messages, Visibility},
    completions::{Chunk, Completions},
    embeddings::EmbeddingSearch,
};
use osy_share::{Event, HandleQuery, SessionInfo};
use tokio::task::JoinSet;

/// API: The user query handler
pub async fn handle_user_query(Paths(sid): Paths<SessionId>, data: Json<HandleQuery>) -> Response {
    let HandleQuery { message } = data.0;

    Response::ok().stream(move |tx| async move {
        let result = match read_session(sid).await {
            Ok((session, messages)) => {
                handle_query(sid, tx.clone(), session, messages, message, false).await
            }
            Err(e) => Err(e),
        };

        if let Err(e) = result {
            error!("[handle_query{{sid={sid}}}] {e}");
            tx.send(Event::Error(str!(e))).ok();
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

/// Handles the user query with direct parallel tool execution and self-healing
#[async_recursion]
#[log(skip_all, fields(sid = %sid))]
async fn handle_query(
    sid: SessionId,
    tx: Sender<Bytes>,
    session: Arc<Mutex<Session>>,
    messages: Arc<Mutex<Messages>>,
    message: Message,
    is_control: bool,
) -> Result<()> {
    // warn!("MESSAGES: {messages:#?}"); // DEBUG
    info!("Processing the user query...");

    let settings = Settings::get();
    let completions_options = settings.completions.options.clone();
    let exec_options = &settings.execution;
    let context_options = &settings.context;

    // 1. RAG: Search for relevant facts about the user and load Session Rules
    let session_guard = session.lock().await;
    let mut facts_prompt = String::new();
    let mut rules_prompt = String::new();

    // Загружаем активные правила сессии (Глобальные + Локальные)
    match session_guard.list_session_rules().await {
        Ok(rules) => {
            if !rules.is_empty() {
                info!(
                    "[Rules] Loaded {} active rules for sid={}",
                    rules.len(),
                    sid
                );
                rules_prompt.push_str(
                    "\n\n### MANDATORY USER RULES & PREFERENCES (STRICTLY FOLLOW THEM):\n",
                );
                for rule in rules {
                    let scope = if rule.is_global { "Global" } else { "Local" };
                    rules_prompt.push_str(&format!(
                        "  * [ID: {}, Scope: {}] {}\n",
                        rule.id, scope, rule.text
                    ));
                }
            } else {
                info!("[Rules] No active rules found for sid={}", sid);
            }
        }
        Err(e) => {
            error!("[Rules] Failed to load session rules: {e}");
        }
    }

    let user_text = context::extract_text_from_msg(&message);
    info!(
        "[RAG] Extracting text from user message: found = {}",
        user_text.is_some()
    );

    if let Some(user_text) = user_text {
        if user_text.trim().is_empty() {
            info!("[RAG] Extracted user text is empty, skipping facts search.");
        } else {
            info!(
                "[RAG] Generating embedding for query (len={})...",
                user_text.len()
            );

            match context::generate_embedding(&user_text, EmbeddingSearch::Query).await {
                Ok(query_vec) => {
                    info!("[RAG] Embedding generated successfully. Querying LanceDB facts...");
                    match session_guard
                        .search_facts(
                            query_vec,
                            context_options.search_limit,
                            context_options.fact_similarity,
                        )
                        .await
                    {
                        Ok(facts) => {
                            if facts.is_empty() {
                                info!(
                                    "[RAG] No facts met the similarity threshold ({}) or DB is empty.",
                                    context_options.fact_similarity
                                );
                            } else {
                                info!(
                                    "[RAG] Loaded {} facts for sid={}: {:?}",
                                    facts.len(),
                                    sid,
                                    facts.iter().map(|f| &f.data.text).collect::<Vec<_>>()
                                );

                                facts_prompt.push_str("\n\n### Loaded User Facts (use them when writing the answer, if necessary):\n");
                                for record in facts {
                                    facts_prompt.push_str(&format!(
                                        "  * [ID: {}] {}\n",
                                        record.id, record.data.text
                                    ));
                                }
                            }
                        }
                        Err(e) => {
                            error!("[RAG] Failed to search facts in LanceDB: {e}");
                        }
                    }
                }
                Err(e) => {
                    error!("[RAG] Failed to generate embedding for text: {e}");
                    tx.send(Event::Error(str!("{e}\n")))?;
                }
            }
        }
    } else {
        warn!(
            "[RAG] Could not extract text content from incoming user Message! Skipping facts search."
        );
    }

    // 2. Preparing the context and system prompts
    let raw_messages = messages.lock().await.messages.clone();
    let base_system_prompt = system_prompt(&session_guard.info, &settings);
    drop(session_guard);

    // --- СОХРАНЯЕМ ВХОДЯЩЕЕ СООБЩЕНИЕ ПОЛЬЗОВАТЕЛЯ В БД ---
    if !is_control {
        session.lock().await.write_message(message.clone()).await?;
    }

    let messages = Messages::from(raw_messages)
        .message(
            Message::system(vec![
                format!("{base_system_prompt}{rules_prompt}{facts_prompt}").into(),
                settings
                    .completions
                    .assist_prompt
                    .replace("{AGENTS_LIST}", &Manager::agents_list_doc().await)
                    .into(),
            ])
            .visibility(Visibility::Internal),
        )
        .message(message)
        .wrap();

    let mut agent_tasks = vec![];
    let mut evals_list = vec![];
    let mut memory_results = vec![];

    let mut retry_count = 0;
    let max_retries = exec_options.max_retries.max(1);

    // LLM Planning cycle
    loop {
        agent_tasks.clear();
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
                    messages.lock().await.add_message(
                        Message::user(vec![
                            format!(
                                "An error occurred: {e}. Please try again to plan using the tools."
                            )
                            .into(),
                        ])
                        .visibility(Visibility::Internal),
                    );
                    continue;
                } else {
                    return Err(e.into());
                }
            }
        };

        // Read AI chunks and collect tool calls
        let mut chunk_error = None;
        while let Some(chunk) = response.next().await {
            match chunk {
                Ok(Chunk::Text(text_part)) => {
                    text_response.push_str(&text_part);
                    tx.send(Event::Answer(text_part))?;
                }

                Ok(Chunk::Tool(tool_call)) => match tool_call.func.name.as_ref() {
                    "handle_task" => match tool_call.parse_args::<skills::task::TaskAction>() {
                        Ok(mut task) => {
                            task.tool_call_id = tool_call.id;
                            agent_tasks.push(task);
                        }
                        Err(e) => {
                            chunk_error = Some(str!("Failed to parse handle_task: {e}").into());
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
                            match skills::fact::handle_remember_fact(&s, act).await {
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
                            match skills::fact::handle_forget_fact(&s, act.fact_id).await {
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

                    "search_fact" => match tool_call.parse_args::<skills::fact::SearchFactAction>()
                    {
                        Ok(act) => {
                            let s = session.lock().await;
                            match skills::fact::handle_search_fact(&s, act).await {
                                Ok(res_msg) => memory_results.push((tool_call.id, res_msg)),
                                Err(e) => {
                                    chunk_error = Some(str!("Failed to search facts: {e}").into());
                                    break;
                                }
                            }
                        }
                        Err(e) => {
                            chunk_error = Some(str!("Failed to parse search_fact: {e}").into());
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
                messages.lock().await.add_message(
                    Message::user(vec![
                        format!(
                            "An error occurred during stream generation: {err}. Please try again."
                        )
                        .into(),
                    ])
                    .visibility(Visibility::Internal),
                );
                continue;
            } else {
                return Err(err);
            }
        }

        // Hallucination check
        if agent_tasks.is_empty()
            && evals_list.is_empty()
            && memory_results.is_empty()
            && text_response.trim().is_empty()
        {
            retry_count += 1;
            if retry_count < max_retries {
                warn!(
                    "Model hallucinated: empty text response and no tool calls. Retrying ({retry_count}/{max_retries})..."
                );
                messages.lock().await.add_message(
                    Message::user(vec![
                        "You returned an empty response. Execute tools or answer the user.".into(),
                    ])
                    .visibility(Visibility::Internal),
                );
                continue;
            } else {
                return Err(str!("Model failed to plan tasks: returned empty response").into());
            }
        }

        break;
    }

    // 3. Process Memory Operations & Notifications
    let has_memory_ops = !memory_results.is_empty();
    if has_memory_ops {
        let mut msg_guard = messages.lock().await;
        for (tool_call_id, res_text) in memory_results {
            tx.send(Event::Thinking(res_text.clone()))?;
            let content_item: Content = format!("Memory Operation Result:\n{res_text}").into();
            msg_guard.push_content(Some(&tool_call_id), content_item);
        }
    }

    // 4. Performing JS calculations
    let has_evals = !evals_list.is_empty();
    if has_evals {
        let results: Vec<(String, Content)> = {
            let mut runtime = Runtime::new();
            evals_list
                .into_iter()
                .map(|(tool_call_id, eval)| {
                    let result: String = match runtime.eval(&eval.code) {
                        Ok(res) => res,
                        Err(e) => format!("JS Execution Error: {e}"),
                    };
                    let content_item: Content = format!("JS Result:\n{result}").into();
                    (tool_call_id, content_item)
                })
                .collect()
        };

        for (tool_call_id, content_item) in results {
            tx.send(Event::Thinking("Executing JS script code...".to_string()))?;
            messages
                .lock()
                .await
                .push_content(Some(&tool_call_id), content_item);
        }
    }

    // 5. Parallel Tool/Agent Execution & Control Step Dispatch
    if !agent_tasks.is_empty() {
        info!("Executing {} agent tasks in parallel", agent_tasks.len());

        let mut workers = JoinSet::new();

        for task in agent_tasks {
            let session = session.clone();
            let messages = messages.clone();
            let tx = tx.clone();

            workers.spawn(
                async move {
                    if let Err(e) = handle_agent(session, messages, tx.clone(), task.clone()).await
                    {
                        error!("[handle_agent] Execution failed: {e}");
                        tx.send(Event::Error(str!("{e}"))).ok();
                    }
                }
                .instrument(Span::current()),
            );
        }

        while let Some(res) = workers.join_next().await {
            if let Err(e) = res {
                error!("Agent task worker panicked: {e}");
            }
        }

        info!("All parallel tasks completed. Launching control query...");
        let control_msg = Message::user(vec![settings.completions.control_prompt.as_str().into()])
            .visibility(Visibility::Internal);

        handle_query(sid, tx, session, messages, control_msg, true).await?;
    } else if has_evals || has_memory_ops {
        info!("JS evaluations or Memory operations finished. Launching control query...");
        let control_msg = Message::user(vec![settings.completions.control_prompt.as_str().into()])
            .visibility(Visibility::Internal);

        handle_query(sid, tx, session, messages, control_msg, true).await?;
    } else {
        tx.send(Event::Finish)?;
        info!("Query processed directly (or control step finished)");

        let to_save = messages
            .lock()
            .await
            .slice(-1)
            .into_iter()
            .filter(|msg| msg.role.is_assistant())
            .collect::<Vec<_>>();
        session.lock().await.write_messages(to_save).await?;
    }

    Ok(())
}

/// Handles an individual agent task
#[log(skip_all, fields(agent = %task.agent, skill = %task.skill))]
pub async fn handle_agent(
    session: Arc<Mutex<Session>>,
    messages: Arc<Mutex<Messages>>,
    tx: Sender<Bytes>,
    task: TaskAction,
) -> Result<()> {
    let agent_name = &task.agent;
    let skill_name = &task.skill;
    let arc_name = arc!(task.agent.to_string());

    // 1. Checking the agent for existence
    let sock_path = match Manager::ensure_agent(&arc_name).await {
        Ok(Some(path)) => path,
        _ => {
            return Err(str!("Agent `{}` is not available or failed to start", agent_name).into());
        }
    };
    let skill_prompt = match Manager::agent_prompt(&arc_name, &skill_name).await {
        Some(prompt) => prompt,
        _ => {
            return Err(str!("Using unknown skill `{}`, aborting...", skill_name).into());
        }
    };

    // 2. Getting tools via IPC
    // warn!("SOCK PATH: {}", sock_path.display()); // DEBUG
    let client = Client::ipc(&sock_path.to_string_lossy());
    let response = client
        .post(&str!("/skills/{}/tools", task.skill))
        .header("Content-Type", "application/json")
        .send()
        .await
        .map_err(|e| str!("Failed to get the `{}` agent tools list: {e}", task.agent))?;
    let tools = response.json::<Vec<anylm::api::Tool>>().await?;

    let log_query = task
        .query
        .chars()
        .take(40)
        .collect::<String>()
        .trim_end_matches('.')
        .replace('\n', " ");

    let msg = str!("Handling `{}` agent: \"{log_query}...\"", agent_name);
    info!("{msg}");
    tx.send(Event::Thinking(msg)).ok();

    let settings = Settings::get();
    let options = settings.completions.options.clone();
    let exec_options = &settings.execution;

    // 3. Creating local context
    let system_pr = system_prompt(&session.lock().await.info, &settings);

    let agent_messages = Messages::new()
        .system(vec![
            system_pr.into(),
            skill_prompt.trim().into(),
        ])
        .user(vec![
            str!("{prompt}\n\n{query}",
                prompt = "For the following request, you MUST use the provided tools and MUST NOT answer from your own knowledge. If no suitable tool is available, return an error explaining that the required tool does not exist. Never invent or assume tools that were not provided.",
                query = task.query
            ).into()
        ])
        .wrap();

    let mut tool_calls = vec![];
    let mut retry_count = 0;
    let max_retries = exec_options.max_retries.max(1);

    // Agent self-healing execution loop
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
                            tx.send(Event::Answer(text_part))?;
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
                            "Stream error. Retrying {} agent execution...",
                            agent_name
                        )))
                        .ok();
                        agent_messages.lock().await.add_user(vec![
                            format!("An error occurred during output generation: {err}. Please try again using tools.").into()
                        ]);
                        continue;
                    } else {
                        return Err(
                            str!("Agent `{agent_name}` failed after stream error: {err}").into(),
                        );
                    }
                }

                if tool_calls.is_empty() && text_response.trim().is_empty() {
                    retry_count += 1;
                    if retry_count < max_retries {
                        warn!(
                            "Agent `{agent_name}` returned empty response and no tool calls. Retrying ({retry_count}/{max_retries})..."
                        );
                        tx.send(Event::Thinking(format!(
                            "Agent {} returned empty response. Retrying task...",
                            agent_name
                        )))
                        .ok();
                        agent_messages.lock().await.add_user(vec![
                            "You did not call any tools. Please execute the requested task using tools now.".into()
                        ]);
                        continue;
                    } else {
                        return Err(str!(
                            "Agent `{agent_name}` failed after {max_retries} retries: empty output"
                        )
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
                    )))
                    .ok();
                    agent_messages.lock().await.add_user(vec![
                        format!("Failed to process request due to error: {e}. Please attempt to execute the task again.").into()
                    ]);
                    continue;
                } else {
                    return Err(str!(
                        "Agent `{agent_name}` failed sending completions request: {e}"
                    )
                    .into());
                }
            }
        }

        if tool_calls.is_empty() {
            break;
        }

        // Parallel execution of sub-tool calls via IPC
        let mut sub_workers = JoinSet::new();

        for tool_call in tool_calls {
            let client = client.clone();
            let sock_path = sock_path.clone();
            let arc_name = arc_name.clone();
            let tx = tx.clone();
            let agent_name = agent_name.to_string();
            let skill_name = skill_name.to_string();

            sub_workers.spawn(
                async move {
                    let func = tool_call.func;
                    let log_json = func.json_str.replace('\n', " ");

                    let msg = format!(
                        "Calling `{agent_name} -> {skill_name}.{}` tool: {log_json}",
                        func.name
                    );
                    info!("{msg}");
                    tx.send(Event::Thinking(msg)).ok();

                    let request_path = format!("/skills/{}/call/{}", skill_name, func.name);
                    let request_body = func.parse_args::<JsonValue>()?;
                    // warn!("RESPONSE: {:?}: {:#?}", &request_path, &request_body); // DEBUG

                    let mut response = client
                        .post(&request_path)
                        .header("Content-Type", "application/json")
                        .json(&request_body)
                        .stream::<Event>()
                        .await;

                    if response.is_err() {
                        warn!(
                            "Agent `{agent_name}` didn't respond. Attempting tactical restart..."
                        );
                        tx.send(Event::Thinking(format!(
                            "Connection lost. Restarting agent {}...",
                            agent_name
                        )))
                        .ok();

                        let _ = Manager::stop(arc_name.clone()).await;

                        if let Ok(Some(_)) = Manager::ensure_agent(&arc_name).await {
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
                                "Agent `{agent_name}` crashed and failed to recover: {e}"
                            )
                            .into());
                        }
                    };

                    let mut full_text = str!();

                    while let Some(event) = stream.recv().await? {
                        match event {
                            Event::Answer(text) => {
                                full_text.push_str(&text);
                            }
                            Event::Thinking(text) => {
                                tx.send(Event::Thinking(text))?;
                            }
                            Event::Error(err) => {
                                tx.send(Event::Error(err))?;
                            }
                            Event::Finish => {}
                        }
                    }

                    Ok::<String, DynError>(full_text)
                }
                .instrument(Span::current()),
            );
        }

        while let Some(worker_result) = sub_workers.join_next().await {
            let full_text: String =
                worker_result.map_err(|e| str!("Worker tool call panicked: {e}"))??;
            let content_item: Content = full_text.into();

            agent_messages
                .lock()
                .await
                .push_content(None, content_item.clone());

            messages
                .lock()
                .await
                .push_content(Some(&task.tool_call_id), content_item);
        }

        break;
    }

    Ok(())
}

/// Generates the system prompt
fn system_prompt(info: &SessionInfo, settings: &Settings) -> String {
    let now_utc = Utc::now();
    let now_local = helpers::now_local(info.timezone);

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
