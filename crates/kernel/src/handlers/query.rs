use crate::{
    manager::Manager,
    prelude::*,
    skills,
    user::{Session, UserState},
    utils,
};

use anylm::{
    api::{Content, Message, Messages, Visibility},
    completions::{Chunk, Completions},
};
use osy_share::{DialogEvent, Event, HandleQuery, Id};
use pearce::Callback;
use rigging::widgets::Confirmation;
use tokio::task::JoinSet;

/// API: User query handler.
pub async fn handle_user_query(Paths(sid): Paths<SessionId>, data: Json<HandleQuery>) -> Response {
    let HandleQuery { message } = data.0;

    Response::ok().stream(move |tx| async move {
        let result = match Session::read(sid).await {
            Ok((session, messages)) => {
                handle_query(sid, tx.clone(), session, messages, message, false, 0).await
            }
            Err(e) => Err(e),
        };

        if let Err(e) = result {
            error!("[handle_query{{sid={sid}}}] {e}");
            tx.send(Event::Error(e.to_string())).ok();
        }
    })
}

/// Handles user query with direct parallel tool execution and self-healing.
#[async_recursion]
#[log(skip_all, fields(sid = %sid))]
async fn handle_query(
    sid: SessionId,
    tx: Sender<Bytes>,
    session: Arc<Mutex<Session>>,
    messages: Arc<Mutex<Messages>>,
    message: Message,
    is_control: bool,
    iteration: usize,
) -> Result<()> {
    info!("Processing the user query (iteration {iteration})...");

    let settings = Settings::get();
    let completions_options = settings.completions.options.clone();
    let exec_options = &settings.execution;
    // let context_options = &settings.context;

    // Цикл повтора ТЕКУЩЕЙ итерации при фатальной ошибке
    'iteration_loop: loop {
        // 1. RAG: Search for relevant facts about the user and load Session Rules
        let session_guard = session.lock().await;
        let mut facts_prompt = String::new();
        let mut rules_prompt = String::new();

        match session_guard.load_rules().await {
            Ok(rules) => {
                if !rules.is_empty() {
                    rules_prompt
                        .push_str("MANDATORY RULES & PREFERENCES (STRICTLY FOLLOW THEM):\n");
                    for rule in rules {
                        let scope = if rule.is_global { "Global" } else { "Local" };
                        rules_prompt.push_str(&format!(
                            "  * [ID: {}, Scope: {}] {}\n",
                            rule.id, scope, rule.text
                        ));
                    }
                }
            }
            Err(e) => error!("[Rules] Failed to load session rules: {e}"),
        }

        let user_text = message.extract_texts().join("\n\n");
        if !user_text.trim().is_empty() {
            match {
                let user = UserState::get_or_init(sid.user_id).await?;
                let user_guard = user.read().await;
                let limit = Settings::get().context.search_limit;
                user_guard.search_facts(&user_text, Some(limit)).await
            } {
                Ok(facts) if !facts.is_empty() => {
                    facts_prompt.push_str("LOADED USER FACTS (USE THEM WHEN NECESSARY):\n");
                    for record in facts {
                        facts_prompt
                            .push_str(&format!("  * [ID: {}] {}\n", record.id, record.data.text));
                    }
                }
                Err(e) => error!("[Facts] Failed to search facts in LanceDB: {e}"),
                _ => {}
            }
        }

        // 2. Preparing the context and system prompts
        let raw_messages = messages.lock().await.messages.clone();
        let base_system_prompt = utils::system_prompt(&session_guard.info, &settings);
        drop(session_guard);

        if !is_control {
            session.lock().await.write_message(message.clone()).await?;
        }

        let messages = Messages::from(raw_messages)
            .message(
                Message::system(vec![
                    format!("{base_system_prompt}\n\n---\n{rules_prompt}\n\n---\n{facts_prompt}")
                        .into(),
                    settings
                        .completions
                        .assist_prompt
                        .replace("{AGENTS_LIST}", &Manager::agents_doc().await)
                        .into(),
                ])
                .visibility(Visibility::Internal),
            )
            .message(message.clone())
            .wrap();

        let mut agent_tasks = vec![];
        let mut evals_list = vec![];
        let mut memory_results = vec![];

        let mut retry_count = 0;
        let max_retries = exec_options.max_retries.max(1);

        // LLM Planning cycle
        let planning_res: Result<()> = loop {
            agent_tasks.clear();
            evals_list.clear();
            memory_results.clear();
            let mut text_response = str!();

            if tx.is_closed() {
                return Err(Error::ConnectionClosed.into());
            }

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
                            "Failed to send query completions request ({retry_count}/{max_retries}): {e}"
                        );
                        messages.lock().await.add_message(
                            Message::user(vec![
                                format!("An error occurred: {e}. Please try again to plan using the tools.").into(),
                            ])
                            .visibility(Visibility::Internal),
                        );
                        continue;
                    } else {
                        break Err(e.into());
                    }
                }
            };

            let mut chunk_error = None;
            while let Some(chunk) = tokio::select! {
                _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
                chunk = response.next() => chunk,
            } {
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
                                chunk_error =
                                    Some(format!("Failed to parse handle_task: {e}").into());
                                break;
                            }
                        },
                        "js_eval" => match tool_call.parse_args::<skills::eval::EvalAction>() {
                            Ok(eval) => evals_list.push((tool_call.id, eval)),
                            Err(e) => {
                                chunk_error = Some(format!("Failed to parse js_eval: {e}").into());
                                break;
                            }
                        },
                        "remember_fact" => match tool_call.parse_args::<osy_share::SetQuery>() {
                            Ok(act) => {
                                match {
                                    let user = UserState::get_or_init(sid.user_id).await?;
                                    let user_guard = user.read().await;
                                    skills::fact::handle_remember_fact(&user_guard, act).await
                                } {
                                    Ok(res_msg) => memory_results.push((tool_call.id, res_msg)),
                                    Err(e) => {
                                        chunk_error =
                                            Some(format!("Failed to save fact: {e}").into());
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                chunk_error =
                                    Some(format!("Failed to parse remember_fact: {e}").into());
                                break;
                            }
                        },
                        "search_fact" => match tool_call.parse_args::<osy_share::SearchQuery>() {
                            Ok(act) => {
                                match {
                                    let user = UserState::get_or_init(sid.user_id).await?;
                                    let user_guard = user.read().await;
                                    skills::fact::handle_search_fact(&user_guard, act).await
                                } {
                                    Ok(res_msg) => memory_results.push((tool_call.id, res_msg)),
                                    Err(e) => {
                                        chunk_error =
                                            Some(format!("Failed to search facts: {e}").into());
                                        break;
                                    }
                                }
                            }
                            Err(e) => {
                                chunk_error =
                                    Some(format!("Failed to parse search_fact: {e}").into());
                                break;
                            }
                        },
                        name => warn!("Unknown tool call `{name}`"),
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
                            format!("An error occurred during stream generation: {err}. Please try again.").into(),
                        ])
                        .visibility(Visibility::Internal),
                    );
                    continue;
                } else {
                    break Err(err);
                }
            }

            if agent_tasks.is_empty()
                && evals_list.is_empty()
                && memory_results.is_empty()
                && text_response.trim().is_empty()
            {
                retry_count += 1;
                if retry_count < max_retries {
                    messages.lock().await.add_message(
                        Message::user(vec![
                            "You returned an empty response. Execute tools or answer the user."
                                .into(),
                        ])
                        .visibility(Visibility::Internal),
                    );
                    continue;
                } else {
                    break Err(
                        format!("Model failed to plan tasks: returned empty response").into(),
                    );
                }
            }

            break Ok(());
        };

        // Если планирование провалилось после всех retries:
        if let Err(e) = planning_res {
            tx.send(Event::Error(e.to_string())).ok();
            let prompt =
                format!("Планирование завершилось с ошибкой: {e}. Хотите попробовать снова?");
            if ask_retry(&tx, prompt).await.unwrap_or(false) {
                info!(
                    "User requested retry after planning error. Retrying iteration {iteration}..."
                );
                continue 'iteration_loop;
            } else {
                return Err(e);
            }
        }

        // 3. Process Memory Operations & Notifications
        if !memory_results.is_empty() {
            let mut msg_guard = messages.lock().await;
            for (tool_call_id, res_text) in memory_results {
                tx.send(Event::Thinking(res_text.clone()))?;
                let content_item: Content = format!("Memory Operation Result:\n{res_text}").into();
                msg_guard.push_content(Some(&tool_call_id), content_item);
            }
        }

        // 4. Performing JS calculations
        let has_evals = !evals_list.is_empty();
        if !evals_list.is_empty() {
            let mut results: Vec<(String, String)> = vec![];
            for (tool_call_id, eval) in evals_list {
                tx.send(Event::Thinking(format!(
                    "Executing JavaScript code: {:80}...",
                    &eval.code
                )))?;
                let result = skills::eval::handle_eval(eval).await?;
                results.push((tool_call_id, format!("JS Result:\n{result}").into()));
            }

            for (tool_call_id, content_item) in results {
                messages
                    .lock()
                    .await
                    .push_content(Some(&tool_call_id), content_item);
            }
        }

        // 5. Parallel Tool/Agent Execution
        let mut execution_error = None;
        let has_tasks = !agent_tasks.is_empty();
        if has_tasks {
            info!("Executing {} agent tasks in parallel", agent_tasks.len());
            let mut workers = JoinSet::new();

            for task in agent_tasks {
                let session = session.clone();
                let messages = messages.clone();
                let tx = tx.clone();

                workers.spawn(
                    async move { skills::task::handle_task(tx, session, messages, task).await }
                        .instrument(Span::current()),
                );
            }

            loop {
                tokio::select! {
                    _ = tx.closed() => {
                        warn!("Client disconnected, aborting agent execution");
                        workers.abort_all();
                        return Err(Error::ConnectionClosed.into());
                    }
                    maybe_res = workers.join_next() => {
                        match maybe_res {
                            Some(Ok(Ok(_))) => {}
                            Some(Ok(Err(e))) => {
                                error!("[handle_agent] Execution failed: {e}");
                                execution_error = Some(e);
                            }
                            Some(Err(e)) => {
                                error!("Agent task worker panicked: {e}");
                                execution_error = Some(Error::Custom(e.to_string()).into());
                            }
                            None => break,
                        }
                    }
                }
            }
        }

        // Если выполнение тасок упало после max_retries внутри handle_task
        if let Some(err) = execution_error {
            tx.send(Event::Error(err.to_string())).ok();
            let prompt =
                format!("Обработка задач завершилась с ошибкой: {err}. Хотите попробовать снова?");
            if ask_retry(&tx, prompt).await.unwrap_or(false) {
                info!("User requested retry after task error. Retrying iteration {iteration}...");
                continue 'iteration_loop;
            } else {
                return Err(err);
            }
        }

        // 6. Recursion / Control Step Dispatch
        let had_actions = has_tasks || has_evals;

        if !is_control && had_actions {
            if iteration + 1 >= exec_options.max_iterations {
                warn!(
                    "Reached maximum allowed iterations ({}), stopping recursive execution.",
                    exec_options.max_iterations
                );
                tx.send(Event::Finish)?;

                let to_save = messages
                    .lock()
                    .await
                    .slice(-1)
                    .into_iter()
                    .filter(|msg| msg.role.is_assistant())
                    .collect::<Vec<_>>();
                session.lock().await.write_messages(to_save).await?;
            } else {
                info!(
                    "Sub-tasks finished. Launching control query (iteration {})...",
                    iteration + 1
                );
                let control_msg =
                    Message::user(vec![settings.completions.control_prompt.as_str().into()])
                        .visibility(Visibility::Internal);

                handle_query(sid, tx, session, messages, control_msg, true, iteration + 1).await?;
            }
        } else {
            // Если это был контрольный запрос ИЛИ действий не было (прямой ответ пользователю)
            tx.send(Event::Finish)?;
            info!("Query processing finished completely.");

            let to_save = messages
                .lock()
                .await
                .slice(-1)
                .into_iter()
                .filter(|msg| msg.role.is_assistant())
                .collect::<Vec<_>>();
            session.lock().await.write_messages(to_save).await?;
        }

        break 'iteration_loop;
    }

    Ok(())
}

/// Requesting confirmation from the user to try again in case of an error.
async fn ask_retry(tx: &Sender<Bytes>, prompt: String) -> Result<bool> {
    let id = Id::new().to_string();
    let mut callback = Callback::register(&id).await;

    tx.send(Event::Dialog(DialogEvent::Confirm {
        id,
        prompt,
        default: Some(Confirmation::Yes),
    }))?;

    let response = tokio::select! {
        _ = tx.closed() => return Err(Error::ConnectionClosed.into()),
        res = callback.recv::<bool>(Duration::from_secs(300)) => res,
    };

    Ok(response.unwrap_or(Some(false)).unwrap_or(false))
}
