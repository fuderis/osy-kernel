use super::{error, info, success};
use crate::{context::extract_text_from_msg, prelude::*};

use anylm::api::{Message, Messages, Role, Visibility};
use chrono::Local;
use osy_share::{
    CompactQuery, Event, HandleQuery, ListQuery, RemoveQuery, SearchQuery, SessionId, SetQuery,
    UserFact, UserRule,
};
use rigging::{
    Stylize,
    style::{Align, BorderStyle, LineStyle, Margin, Padding, SpinnerStyle},
    widgets::{Input, Text},
};
use std::{error::Error, process::Command, sync::Arc};
use tokio::sync::Mutex;

const MIN_WIDTH: usize = 80;
const INPUT_MAX_HEIGHT: usize = 20;
const USER_ID: u128 = 0;

/// Handles the interactive CLI chat session lifecycle.
pub async fn handle_chat(load_history: bool) -> Result<()> {
    // initialize base endpoint address and tcp client
    let port = Settings::get().server.port;
    let base_url = str!("http://127.0.0.1:{port}");
    let client = Client::tcp();

    // -----------------------------------------------------------------
    // 1. Healthcheck & Server Auto-start
    // -----------------------------------------------------------------

    // verify whether the backend server is reachable
    if client
        .get(&str!("{base_url}/refresh"))
        .send()
        .await
        .is_err()
    {
        // attempt to auto-start backend process if offline
        if Command::new(path!("$")).arg("start").spawn().is_ok() {
            let ping_url = str!("{base_url}/ping");
            let mut is_ok = false;

            // poll status endpoint until service responds or times out
            for _ in 0..10 {
                tokio::time::sleep(Duration::from_millis(500)).await;
                if client.get(&ping_url).send().await.is_ok() {
                    is_ok = true;
                    break;
                }
            }

            // report timeout if service fails to respond
            if !is_ok {
                eprintln!(
                    "{}: Server started but is not responding.",
                    "Timeout".red().bold()
                );
            }
        } else {
            // log process spawning failure
            eprintln!("{}: Failed to execute server", "Error".red().bold());
        }
    }

    // -----------------------------------------------------------------
    // 2. Session Initialization & History Loading
    // -----------------------------------------------------------------

    // initialize default fallback session id
    let mut session_id = SessionId::new(USER_ID);
    let sessions_url = str!("{base_url}/users/{USER_ID}/sessions");
    let sessions_query = ListQuery { count: Some(1) };

    // fetch existing active session for current user
    if let Ok(res) = client
        .post(&sessions_url)
        .json(&sessions_query)
        .send()
        .await
    {
        if let Ok(active_sessions) = res.json::<Vec<SessionId>>().await {
            if let Some(last_session) = active_sessions.into_iter().next() {
                session_id = last_session;
            }
        }
    }

    // wrap session id in atomic reference counter and async mutex
    let session_id = Arc::new(Mutex::new(session_id));

    // capture local environment metadata closure for backend handshake
    let get_session_info = || {
        let tz_minutes = (chrono::Local::now().offset().local_minus_utc() / 60) as i16;
        osy_share::SessionInfo {
            current_path: std::env::current_dir().ok(),
            timezone: tz_minutes,
        }
    };

    // initialize session on backend and retrieve history payload
    let current_sid = session_id.lock().await.clone();
    let init_res = client
        .post(&str!("{base_url}/sessions/{current_sid}/init"))
        .json(&get_session_info())
        .send()
        .await;

    if let Ok(res) = init_res {
        if let Ok(history) = res.json::<Messages>().await {
            // filter messages: retain only public non-tool messages
            let valid_messages: Vec<&Message> = history
                .messages
                .iter()
                .filter(|msg| msg.visibility == Visibility::Public && !msg.role.is_tool())
                .collect();

            // load active UI color theme configurations
            let cfg = Settings::get();
            let brand_color = cfg.theme.brand_color();
            let bg_color = cfg.theme.bg_color();
            let alt_color = cfg.theme.alt_color();

            if load_history {
                // assemble sequential user and final assistant message pairs
                let mut pairs: Vec<(&Message, Option<&Message>)> = Vec::new();
                let mut current_user_msg: Option<&Message> = None;
                let mut last_assistant_msg: Option<&Message> = None;

                for msg in valid_messages {
                    match msg.role {
                        Role::User => {
                            // save previous user-assistant pair before advancing
                            if let Some(user) = current_user_msg.take() {
                                pairs.push((user, last_assistant_msg.take()));
                            }
                            current_user_msg = Some(msg);
                        }
                        Role::Assistant => {
                            let text = extract_text_from_msg(msg).unwrap_or_default();
                            // bind assistant message only when non-empty textual content exists
                            if !text.trim().is_empty() {
                                if current_user_msg.is_some() {
                                    last_assistant_msg = Some(msg);
                                } else {
                                    // handle standalone system/assistant greetings
                                    pairs.push((msg, None));
                                }
                            }
                        }
                        _ => {}
                    }
                }

                // push final remaining pair to collection
                if let Some(user) = current_user_msg {
                    pairs.push((user, last_assistant_msg));
                }

                // render paired history entries into terminal widgets
                for (user_or_single_msg, assistant_msg) in pairs {
                    let user_text = extract_text_from_msg(user_or_single_msg).unwrap_or_default();

                    let (user_display_text, asst_display_text) =
                        match (user_or_single_msg.role.clone(), assistant_msg) {
                            (Role::User, Some(asst)) => {
                                let asst_text = extract_text_from_msg(asst).unwrap_or_default();
                                (user_text.clone(), asst_text)
                            }
                            (Role::User, None) => (user_text.clone(), String::new()),
                            _ => (user_text.clone(), String::new()),
                        };

                    let timestamp_str = user_or_single_msg
                        .timestamp
                        .map(|ts| {
                            let datetime: chrono::DateTime<Local> = ts.into();
                            datetime.format("%a %I:%M %p").to_string()
                        })
                        .unwrap_or_else(|| Local::now().format("%a %I:%M %p").to_string());

                    Text::new(user_display_text.dim().to_string())
                        .title(format!(" {timestamp_str} ").with(alt_color), Align::TopLeft)
                        .min_width(MIN_WIDTH)
                        .border(BorderStyle::Rounded)
                        .border_color(alt_color)
                        .background(bg_color)
                        .prefix_color(alt_color)
                        .prefix_line(LineStyle::Solid)
                        .bullet_color(alt_color)
                        .code_color(alt_color)
                        .padding(Padding::hor(1))
                        .margin(Margin::default().bottom(1))
                        .handler(async move |handle| {
                            // Здесь передаем ответ ассистента в хэндлер
                            if !asst_display_text.is_empty() {
                                handle.update(asst_display_text);
                            }
                        })
                        .render()
                        .await?;
                }
            } else if !valid_messages.is_empty() {
                // render notification badge showing loaded history message count
                let info_msg = format!(
                    "Loaded {} messages from history.",
                    (valid_messages.len() / 2).max(1)
                );
                Text::new(info_msg.italic().dim().to_string())
                    .min_width(MIN_WIDTH)
                    .border(BorderStyle::Rounded)
                    .border_color(brand_color)
                    .background(bg_color)
                    .prefix_color(alt_color)
                    .padding(Padding::hor(1))
                    .margin(Margin::default().bottom(1))
                    .render()
                    .await?;
            }
        }
    }

    // -----------------------------------------------------------------
    // 3. Main Interactive Loop
    // -----------------------------------------------------------------

    let run_loop = async {
        loop {
            let base_url = base_url.clone();
            let cfg = Settings::get();
            let brand_color = cfg.theme.brand_color();
            let bg_color = cfg.theme.bg_color();
            let alt_color = cfg.theme.alt_color();
            let blink_color = cfg.theme.blink_color();

            // --- Phase A: User Input ---
            // capture multiline user input from interactive terminal widget
            let user_query = Input::new()
                .placeholder("Enter instructions...".with(alt_color))
                .title(" Prompt ".bold().with(brand_color), Align::TopLeft)
                .title(
                    format!(" {} ", cfg.completions.options.model)
                        .bold()
                        .with(brand_color),
                    Align::BottomLeft,
                )
                .title(
                    " [Alt+Enter] Submit ".bold().with(alt_color),
                    Align::BottomRight,
                )
                .use_buffer(0, Some(100))
                .min_width(MIN_WIDTH)
                .max_height(INPUT_MAX_HEIGHT)
                .border(BorderStyle::Rounded)
                .border_color(brand_color)
                .background(bg_color)
                .padding(Padding::hor(1))
                .multiline(true)
                .clear_after(true)
                .render()
                .await?;

            let trimmed = user_query.trim();
            if trimmed.is_empty() {
                continue;
            }

            // --- Command Handling ---
            // evaluate and dispatch slash command instructions
            if trimmed.starts_with('/') {
                let args: Vec<&str> = trimmed.split_whitespace().collect();
                let is_global = args.iter().any(|&a| a == "-g");

                // Главное имя команды (например, "facts", "rules", "remember")
                let cmd = args[0].trim_start_matches('/').to_lowercase();

                // Второе слово (действие), если передано
                let sub_cmd = args.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

                // Чистые аргументы без имени команды, подкоманды и флага -g
                let clean_args: Vec<&str> =
                    args[1..].iter().copied().filter(|&a| a != "-g").collect();

                // Если есть подкоманда (например, "set" в "/facts set"), аргументы нагрузки начинаются с 2-го элемента
                let payload = if !clean_args.is_empty() && clean_args[0].to_lowercase() == sub_cmd {
                    clean_args[1..].join(" ")
                } else {
                    clean_args.join(" ")
                };

                let sid = session_id.lock().await.clone();

                // Хелпер для форматирования длинных строк (безопасно для UTF-8 / кириллицы)
                let truncate = |s: &str, max_len: usize| -> String {
                    let char_count = s.chars().count();
                    if char_count > max_len {
                        let truncated: String = s.chars().take(max_len).collect();
                        format!("\"{truncated}...\"")
                    } else {
                        format!("\"{s}\"")
                    }
                };

                // Хелпер для отображения результатов в UI
                let render_msg = |msg: String| async move {
                    Text::new("")
                        .min_width(MIN_WIDTH)
                        .border(BorderStyle::Rounded)
                        .border_color(brand_color)
                        .background(bg_color)
                        .padding(Padding::hor(1))
                        .margin(Margin::default().bottom(1))
                        .handler(async move |handle| handle.update(msg))
                        .render()
                        .await
                };

                match cmd.as_str() {
                    "exit" | "quit" => break,

                    "help" => {
                        let cmds = [
                            ("help", "Show this help message"),
                            ("new", "Start a new clear session"),
                            ("clear", "Clear remote chat history"),
                            ("clone", "Clone the current session"),
                            ("compact [N]", "Compress context preserving N messages"),
                            ("facts list [count]", "List stored facts"),
                            ("facts add <fact>", "Save a new fact (alias: /remember)"),
                            ("facts search <query>", "Search stored facts"),
                            ("facts remove <id>", "Remove fact by ID (alias: /forget)"),
                            ("facts clear", "Purge all facts"),
                            (
                                "rules list [-g] [count]",
                                "List active rules (-g for global)",
                            ),
                            ("rules set [-g] <rule>", "Set dynamic behavior rule"),
                            ("rules remove [-g] <id>", "Remove specific rule by ID"),
                            ("rules clear [-g]", "Clear defined rules"),
                            ("exit", "Exit the application"),
                        ];

                        let max_len = cmds.iter().map(|(c, _)| c.len()).max().unwrap_or(0) + 2;
                        let formatted_cmds: Vec<String> = cmds
                            .into_iter()
                            .map(|(c, desc)| {
                                format!(
                                    "  {}{}{}",
                                    "/".with(alt_color),
                                    format!("{:<width$}", c, width = max_len).with(alt_color),
                                    desc,
                                )
                            })
                            .collect();

                        let help_text = format!(
                            "{}\n{}",
                            "Available Commands:".bold().with(brand_color),
                            formatted_cmds
                                .join("\n")
                                .replace('<', "&lt;")
                                .replace('>', "&gt;")
                        );

                        render_msg(help_text).await?;
                        continue;
                    }

                    // --- Memory: Facts ---
                    "facts" | "fact" => match sub_cmd.as_str() {
                        "list" | "ls" => {
                            let count = clean_args
                                .get(1)
                                .and_then(|a| a.parse::<usize>().ok())
                                .or(Some(20));

                            let endpoint = format!("{base_url}/users/{USER_ID}/facts/list");
                            let res = client
                                .post(&endpoint)
                                .json(&ListQuery { count })
                                .send()
                                .await;

                            let content = match res {
                                Ok(r) => {
                                    let status = r.status();
                                    if status.is_success() {
                                        match r.json::<Vec<UserFact>>().await {
                                            Ok(facts) if facts.is_empty() => {
                                                "No stored facts found.".to_string()
                                            }
                                            Ok(facts) => format!(
                                                "Stored facts:\n{}",
                                                facts
                                                    .iter()
                                                    .map(|f| format!("• [{}] {}", f.id, f.text))
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            ),
                                            Err(e) => format!("Failed to parse JSON response: {e}"),
                                        }
                                    } else {
                                        let err_body = r.text().await.unwrap_or_default();
                                        format!("List facts failed [{status}]: {err_body}")
                                    }
                                }
                                Err(e) => format!("Network error querying facts backend: {e}"),
                            };

                            render_msg(content).await?;
                            continue;
                        }

                        "set" | "add" | "remember" => {
                            if payload.is_empty() {
                                render_msg("Usage: /facts set <text>".into()).await?;
                                continue;
                            }
                            let preview = truncate(&payload, 40);
                            let endpoint = format!("{base_url}/users/{USER_ID}/facts/set");

                            let msg = match client
                                .post(&endpoint)
                                .json(&SetQuery {
                                    id: None,
                                    text: payload,
                                })
                                .send()
                                .await
                            {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        match res.json::<UserFact>().await {
                                            Ok(fact) => format!(
                                                "Saved to global memory: [{}] {}",
                                                fact.id,
                                                truncate(&fact.text, 40)
                                            ),
                                            Err(_) => format!("Saved to global memory: {preview}"),
                                        }
                                    } else {
                                        let err_body = res.text().await.unwrap_or_default();
                                        if err_body.trim().is_empty() {
                                            format!("Server returned status code: {status}")
                                        } else {
                                            format!("Error [{status}]: {err_body}")
                                        }
                                    }
                                }
                                Err(e) => format!("Network/Transport error: {e}"),
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        "remove" | "rm" | "forget" | "del" => {
                            let trimmed_payload = payload.trim();
                            if trimmed_payload.is_empty() {
                                render_msg("Usage: /facts remove <id>".into()).await?;
                                continue;
                            }

                            let fact_id: u64 = match trimmed_payload.parse() {
                                Ok(id) => id,
                                Err(_) => {
                                    render_msg(format!(
                                        "Invalid ID '{trimmed_payload}'. Must be a numeric u64 ID."
                                    ))
                                    .await?;
                                    continue;
                                }
                            };

                            let endpoint = format!("{base_url}/users/{USER_ID}/facts/remove");

                            let msg = match client
                                .post(&endpoint)
                                .json(&RemoveQuery { id: fact_id })
                                .send()
                                .await
                            {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        format!("Removed fact #{fact_id}")
                                    } else {
                                        let err_body = res.text().await.unwrap_or_default();
                                        if err_body.trim().is_empty() {
                                            format!("Server returned status code: {status}")
                                        } else {
                                            format!("Error [{status}]: {err_body}")
                                        }
                                    }
                                }
                                Err(e) => format!("Network/Transport error: {e}"),
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        "search" | "find" => {
                            if payload.is_empty() {
                                render_msg("Usage: /facts search <query>".into()).await?;
                                continue;
                            }

                            let endpoint = format!("{base_url}/users/{USER_ID}/facts/search");
                            let res = client
                                .post(&endpoint)
                                .json(&SearchQuery { query: payload })
                                .send()
                                .await;

                            let content = match res {
                                Ok(r) => {
                                    let status = r.status();
                                    if status.is_success() {
                                        match r.text().await {
                                            Ok(body) => {
                                                if let Ok(facts) =
                                                    serde_json::from_str::<Vec<UserFact>>(&body)
                                                {
                                                    if facts.is_empty() {
                                                        "No matching facts found.".to_string()
                                                    } else {
                                                        format!(
                                                            "Found facts:\n{}",
                                                            facts
                                                                .iter()
                                                                .map(|f| format!(
                                                                    "• [{}] {}",
                                                                    f.id, f.text
                                                                ))
                                                                .collect::<Vec<_>>()
                                                                .join("\n")
                                                        )
                                                    }
                                                } else if let Ok(facts) =
                                                    serde_json::from_str::<Vec<String>>(&body)
                                                {
                                                    if facts.is_empty() {
                                                        "No matching facts found.".to_string()
                                                    } else {
                                                        format!(
                                                            "Found facts:\n{}",
                                                            facts
                                                                .iter()
                                                                .map(|f| format!("• {f}"))
                                                                .collect::<Vec<_>>()
                                                                .join("\n")
                                                        )
                                                    }
                                                } else {
                                                    format!("Failed to parse JSON response: {body}")
                                                }
                                            }
                                            Err(e) => format!("Failed to read response body: {e}"),
                                        }
                                    } else {
                                        let err_body = r.text().await.unwrap_or_default();
                                        format!("Search failed [{status}]: {err_body}")
                                    }
                                }
                                Err(e) => format!("Network error querying facts backend: {e}"),
                            };

                            render_msg(content).await?;
                            continue;
                        }

                        "clear" | "purge" => {
                            let endpoint = format!("{base_url}/users/{USER_ID}/facts/clear");

                            let msg = match client.post(&endpoint).send().await {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        "All global facts cleared.".to_string()
                                    } else {
                                        let err_body = res.text().await.unwrap_or_default();
                                        if err_body.trim().is_empty() {
                                            format!("Server returned status code: {status}")
                                        } else {
                                            format!("Error [{status}]: {err_body}")
                                        }
                                    }
                                }
                                Err(e) => format!("Network/Transport error: {e}"),
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        _ => {
                            render_msg("Unknown facts subcommand. Available: list, add, remove, search, clear".into()).await?;
                            continue;
                        }
                    },

                    // Удобные алиасы
                    "remember" => {
                        if payload.is_empty() {
                            render_msg("Usage: /remember <text>".into()).await?;
                            continue;
                        }
                        let preview = truncate(&payload, 40);
                        let endpoint = format!("{base_url}/users/{USER_ID}/facts/set");

                        let msg = match client
                            .post(&endpoint)
                            .json(&SetQuery {
                                id: None,
                                text: payload,
                            })
                            .send()
                            .await
                        {
                            Ok(res) => {
                                let status = res.status();
                                if status.is_success() {
                                    match res.json::<UserFact>().await {
                                        Ok(fact) => format!(
                                            "Saved to global memory: [{}] {}",
                                            fact.id,
                                            truncate(&fact.text, 40)
                                        ),
                                        Err(_) => format!("Saved to global memory: {preview}"),
                                    }
                                } else {
                                    let err_body = res.text().await.unwrap_or_default();
                                    if err_body.trim().is_empty() {
                                        format!("Server returned status code: {status}")
                                    } else {
                                        format!("Error [{status}]: {err_body}")
                                    }
                                }
                            }
                            Err(e) => format!("Network/Transport error: {e}"),
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    "forget" => {
                        let trimmed_payload = payload.trim();
                        if trimmed_payload.is_empty() {
                            render_msg("Usage: /forget <id>".into()).await?;
                            continue;
                        }

                        let fact_id: u64 = match trimmed_payload.parse() {
                            Ok(id) => id,
                            Err(_) => {
                                render_msg(format!(
                                    "Invalid ID '{trimmed_payload}'. Must be a numeric u64 ID."
                                ))
                                .await?;
                                continue;
                            }
                        };

                        let endpoint = format!("{base_url}/users/{USER_ID}/facts/remove");

                        let msg = match client
                            .post(&endpoint)
                            .json(&RemoveQuery { id: fact_id })
                            .send()
                            .await
                        {
                            Ok(res) => {
                                let status = res.status();
                                if status.is_success() {
                                    format!("Removed fact #{fact_id}")
                                } else {
                                    let err_body = res.text().await.unwrap_or_default();
                                    if err_body.trim().is_empty() {
                                        format!("Server returned status code: {status}")
                                    } else {
                                        format!("Error [{status}]: {err_body}")
                                    }
                                }
                            }
                            Err(e) => format!("Network/Transport error: {e}"),
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    // --- Memory: Rules ---
                    "rules" | "rule" => match sub_cmd.as_str() {
                        "list" | "ls" => {
                            let count = clean_args
                                .get(1)
                                .and_then(|a| a.parse::<usize>().ok())
                                .or(Some(20));

                            let endpoint = if is_global {
                                format!("{base_url}/users/{USER_ID}/rules/list")
                            } else {
                                format!("{base_url}/sessions/{sid}/rules/list")
                            };

                            let scope_str = if is_global { "global" } else { "session" };

                            let res = client
                                .post(&endpoint)
                                .json(&ListQuery { count })
                                .send()
                                .await;

                            let content = match res {
                                Ok(r) => {
                                    let status = r.status();
                                    if status.is_success() {
                                        match r.json::<Vec<UserRule>>().await {
                                            Ok(rules) if rules.is_empty() => {
                                                format!("No active {scope_str} rules found.")
                                            }
                                            Ok(rules) => format!(
                                                "Active {scope_str} rules:\n{}",
                                                rules
                                                    .iter()
                                                    .map(|rule| format!(
                                                        "• [{}] {}",
                                                        rule.id, rule.text
                                                    ))
                                                    .collect::<Vec<_>>()
                                                    .join("\n")
                                            ),
                                            Err(e) => format!("Failed to parse JSON response: {e}"),
                                        }
                                    } else {
                                        let err_body = r.text().await.unwrap_or_default();
                                        format!("List rules failed [{status}]: {err_body}")
                                    }
                                }
                                Err(e) => format!("Network error querying rules backend: {e}"),
                            };

                            render_msg(content).await?;
                            continue;
                        }

                        "set" | "add" => {
                            if payload.is_empty() {
                                render_msg("Usage: /rules set [-g] <rule text>".into()).await?;
                                continue;
                            }

                            let endpoint = if is_global {
                                format!("{base_url}/users/{USER_ID}/rules/set")
                            } else {
                                format!("{base_url}/sessions/{sid}/rules/set")
                            };

                            let scope_str = if is_global { "global" } else { "session" };
                            let preview = truncate(&payload, 40);

                            let msg = match client
                                .post(&endpoint)
                                .json(&SetQuery {
                                    id: None,
                                    text: payload,
                                })
                                .send()
                                .await
                            {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        match res.json::<UserRule>().await {
                                            Ok(rule) => format!(
                                                "Applied {scope_str} rule: [{}] {}",
                                                rule.id,
                                                truncate(&rule.text, 40)
                                            ),
                                            Err(_) => {
                                                format!("Applied {scope_str} rule: {preview}")
                                            }
                                        }
                                    } else {
                                        let err_body = res.text().await.unwrap_or_default();
                                        if err_body.trim().is_empty() {
                                            format!("Server returned status code: {status}")
                                        } else {
                                            format!("Error [{status}]: {err_body}")
                                        }
                                    }
                                }
                                Err(e) => format!("Network/Transport error: {e}"),
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        "remove" | "rm" | "del" => {
                            let trimmed_payload = payload.trim();
                            if trimmed_payload.is_empty() {
                                render_msg("Usage: /rules remove [-g] <id>".into()).await?;
                                continue;
                            }

                            let rule_id: u64 = match trimmed_payload.parse() {
                                Ok(id) => id,
                                Err(_) => {
                                    render_msg(format!(
                                        "Invalid ID '{trimmed_payload}'. Must be a numeric u64 ID."
                                    ))
                                    .await?;
                                    continue;
                                }
                            };

                            let endpoint = if is_global {
                                format!("{base_url}/users/{USER_ID}/rules/remove")
                            } else {
                                format!("{base_url}/sessions/{sid}/rules/remove")
                            };

                            let scope_str = if is_global { "global" } else { "session" };

                            let msg = match client
                                .post(&endpoint)
                                .json(&RemoveQuery { id: rule_id })
                                .send()
                                .await
                            {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        format!("Removed {scope_str} rule #{rule_id}")
                                    } else {
                                        let err_body = res.text().await.unwrap_or_default();
                                        if err_body.trim().is_empty() {
                                            format!("Server returned status code: {status}")
                                        } else {
                                            format!("Error [{status}]: {err_body}")
                                        }
                                    }
                                }
                                Err(e) => format!("Network/Transport error: {e}"),
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        "clear" | "purge" => {
                            let endpoint = if is_global {
                                format!("{base_url}/users/{USER_ID}/rules/clear")
                            } else {
                                format!("{base_url}/sessions/{sid}/rules/clear")
                            };

                            let scope_str = if is_global { "global" } else { "session" };

                            let msg = match client.post(&endpoint).send().await {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        format!("All {scope_str} rules cleared.")
                                    } else {
                                        let err_body = res.text().await.unwrap_or_default();
                                        if err_body.trim().is_empty() {
                                            format!("Server returned status code: {status}")
                                        } else {
                                            format!("Error [{status}]: {err_body}")
                                        }
                                    }
                                }
                                Err(e) => format!("Network/Transport error: {e}"),
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        _ => {
                            render_msg(
                                "Unknown rules subcommand. Available: list, set, remove, clear"
                                    .into(),
                            )
                            .await?;
                            continue;
                        }
                    },

                    "new" => {
                        let new_sid = SessionId::new(USER_ID);
                        *session_id.lock().await = new_sid.clone();

                        let msg = match client
                            .post(&format!("{base_url}/sessions/{new_sid}/init"))
                            .json(&get_session_info())
                            .send()
                            .await
                        {
                            Ok(res) => {
                                let status = res.status();
                                if status.is_success() {
                                    format!("Started new session: {new_sid}")
                                } else {
                                    let err_body = res.text().await.unwrap_or_default();
                                    if err_body.trim().is_empty() {
                                        format!("Server returned status code: {status}")
                                    } else {
                                        format!("Error [{status}]: {err_body}")
                                    }
                                }
                            }
                            Err(e) => format!("Network/Transport error: {e}"),
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    "clone" | "fork" => {
                        // 1. Получаем ID текущей (исходной) сессии
                        let old_sid = session_id.lock().await.clone();

                        // 2. Вспомогательная структура для десериализации ответа сервера
                        #[derive(serde::Deserialize)]
                        struct CloneResponse {
                            id: SessionId,
                        }

                        // 3. Отправляем POST-запрос на клонирование
                        let msg = match client
                            .post(&format!("{base_url}/sessions/{old_sid}/clone"))
                            .send()
                            .await
                        {
                            Ok(res) => {
                                let status = res.status();
                                if status.is_success() {
                                    // 4. Считываем JSON с новым ID из ответа сервера
                                    match res.json::<CloneResponse>().await {
                                        Ok(payload) => {
                                            let new_sid = payload.id;

                                            // 5. Обновляем локальный ID сессии
                                            *session_id.lock().await = new_sid.clone();

                                            // Отправляем сигнал на закрытие предыдущей сессии
                                            client
                                                .post(&format!(
                                                    "{base_url}/sessions/{old_sid}/finish"
                                                ))
                                                .send()
                                                .await?;

                                            format!(
                                                "Cloned current context into new session: {new_sid}"
                                            )
                                        }
                                        Err(e) => {
                                            format!("Failed to parse clone response JSON: {e}")
                                        }
                                    }
                                } else {
                                    let err_body = res.text().await.unwrap_or_default();
                                    if err_body.trim().is_empty() {
                                        format!(
                                            "Failed to clone session. Server returned status: {status}"
                                        )
                                    } else {
                                        format!("Error cloning session [{status}]: {err_body}")
                                    }
                                }
                            }
                            Err(e) => format!("Network/Transport error during clone: {e}"),
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    "clear" | "clean" => {
                        let sid = session_id.lock().await.clone();
                        let endpoint = format!("{base_url}/sessions/{sid}/clear");

                        let msg = match client.post(&endpoint).send().await {
                            Ok(res) => {
                                let status = res.status();
                                if status.is_success() {
                                    "History cleared successfully.".to_string()
                                } else {
                                    let err_body = res.text().await.unwrap_or_default();
                                    if err_body.trim().is_empty() {
                                        format!("Server returned status code: {status}")
                                    } else {
                                        format!("Error [{status}]: {err_body}")
                                    }
                                }
                            }
                            Err(e) => format!("Network/Transport error: {e}"),
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    "compact" | "compress" => {
                        let sid = session_id.lock().await.clone();
                        let preserve = args
                            .get(1)
                            .and_then(|i| i.parse::<usize>().ok())
                            .unwrap_or_else(|| Settings::get().execution.preserve_messages);

                        Text::new("Compressing context...")
                            .title(" Thinking... ".bold().with(brand_color), Align::TopLeft)
                            .min_width(MIN_WIDTH)
                            .spinner_style(SpinnerStyle::Dots)
                            .spinner_color(brand_color)
                            .border(BorderStyle::Rounded)
                            .border_color(brand_color)
                            .background(bg_color)
                            .padding(Padding::hor(1))
                            .margin(Margin::default().bottom(1))
                            .handler(move |handle| async move {
                                let endpoint = format!("{base_url}/sessions/{sid}/compact");
                                let res = Client::tcp()
                                    .post(&endpoint)
                                    .json(&CompactQuery {
                                        preserve: Some(preserve),
                                    })
                                    .stream::<Event>()
                                    .await;

                                match res {
                                    Ok(mut stream) => {
                                        let mut summary = String::new();
                                        while let Ok(Some(event)) = stream.recv().await {
                                            match event {
                                                Event::Thinking(status) => {
                                                    handle.update(format!("[{status}]"));
                                                }
                                                Event::Answer(text) => {
                                                    summary.push_str(&text);
                                                    handle.update(summary.clone());
                                                }
                                                Event::Error(err) => {
                                                    handle.update(format!(
                                                        "Compression error: {err}"
                                                    ));
                                                }
                                                Event::Finish => break,
                                            }
                                        }
                                    }
                                    Err(e) => handle.update(format!("Network error: {e}")),
                                }
                            })
                            .render()
                            .await?;
                        continue;
                    }

                    _ => {}
                }
            }

            // --- Phase B: Streaming Response (User Query + AI Output) ---
            // prepare query request payload and UI metadata
            let sid = session_id.lock().await.clone();
            let timestamp = Local::now().format("%a %I:%M %p").to_string();
            let query_msg = Message::user(vec![trimmed.into()]);

            // render active prompt box and stream completion events from server
            Text::new(format!("{}", trimmed.dim()))
                .title(format!(" {timestamp} ").with(alt_color), Align::TopLeft)
                .min_width(MIN_WIDTH)
                .spinner_style(SpinnerStyle::MiniDots)
                .spinner_color(brand_color)
                .prefix_color(alt_color)
                .prefix_line(LineStyle::Solid)
                .border(BorderStyle::Rounded)
                .border_color(alt_color)
                .background(bg_color)
                .padding(Padding::hor(1))
                .margin(Margin::default().bottom(1))
                .handler(move |handle| async move {
                    // establish async SSE event stream connection with server
                    let res = Client::tcp()
                        .post(&str!("{base_url}/sessions/{sid}/query"))
                        .json(&HandleQuery { message: query_msg })
                        .stream::<Event>()
                        .await;

                    match res {
                        Ok(mut stream) => {
                            let mut full_response = String::new();

                            // process streaming tokens and update widget buffer in real time
                            while let Ok(Some(event)) = stream.recv().await {
                                match event {
                                    Event::Thinking(status) => {
                                        if full_response.is_empty() {
                                            handle.update(format!("{}", status.italic().dim()));
                                        }
                                    }
                                    Event::Answer(chunk) => {
                                        full_response.push_str(&chunk);
                                        handle.update(full_response.clone());
                                    }
                                    Event::Error(err) => {
                                        handle.update(format!("\n[ERROR] {err}"));
                                    }
                                    Event::Finish => break,
                                }
                            }
                        }
                        Err(err) => {
                            handle.update(format!("\n[ERROR] Connection failed: {err}"));
                        }
                    }
                })
                .blink_color(blink_color)
                .prefix_color(alt_color)
                .bullet_color(alt_color)
                .code_color(alt_color)
                .render()
                .await?;
        }

        Ok::<(), Box<dyn Error + Send + Sync>>(())
    };

    let res = run_loop.await;

    // -----------------------------------------------------------------
    // 4. Graceful Shutdown
    // -----------------------------------------------------------------
    // flush backend state and finalize active chat session cleanly
    info("", "Flushing DB records and closing session cleanly...");
    let final_sid = session_id.lock().await.clone();
    let finish_url = format!("http://127.0.0.1:{port}/sessions/{final_sid}/finish");

    if let Err(e) = Client::tcp().post(&finish_url).send().await {
        error(str!("Warning: Failed to finish session cleanly on backend: {e}").into());
    } else {
        success("Session closed successfully.");
    }

    res
}
