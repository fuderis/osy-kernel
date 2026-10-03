use crate::prelude::*;

use osy_share::{
    CompactQuery, Event, ListQuery, RemoveQuery, RenameQuery, SearchQuery, SessionId, SetQuery,
    UserFact, UserRule,
};
use rigging::{
    Stylize,
    style::{Align, SpinnerStyle},
};

#[derive(serde::Deserialize)]
struct CloneResponse {
    id: SessionId,
}

/// Helper for safe string truncation (UTF-8 / Cyrillic aware).
pub fn truncate(s: &str, max_len: usize) -> String {
    let char_count = s.chars().count();
    if char_count > max_len {
        let truncated: String = s.chars().take(max_len).collect();
        format!("\"{truncated}...\"")
    } else {
        format!("\"{s}\"")
    }
}

/// Renders a message in the interactive text widget.
pub async fn render_msg(msg: impl Into<String>) -> Result<()> {
    let msg = msg.into();
    super::text_widget("")
        .handler(move |mut ctx| async move {
            *ctx.state = msg;
            ctx.finish();
        })
        .render()
        .await
        .map_err(|e| e.into())
}

/// Generic helper to perform POST requests and handle HTTP/network errors consistently.
async fn send_post<T: serde::Serialize>(
    client: &Client,
    url: &str,
    json: Option<&T>,
) -> StdResult<pearce::client::Response, String> {
    let mut req = client.post(url);
    if let Some(body) = json {
        req = req.json(body);
    }
    match req.send().await {
        Ok(res) => {
            let status = res.status();
            if status.is_success() {
                Ok(res)
            } else {
                let err_body = res.text().await.unwrap_or_default();
                if err_body.trim().is_empty() {
                    Err(format!("Server returned status code: {status}"))
                } else {
                    Err(format!("Error [{status}]: {err_body}"))
                }
            }
        }
        Err(e) => Err(format!("Network/Transport error: {e}")),
    }
}

/// Renders CLI input widget.
pub async fn render_input(
    client: &Client,
    base_url: String,
    session_id: &State<SessionId>,
) -> Result<String> {
    loop {
        let cfg = Config::get();
        let brand_color = cfg.theme.brand_color();
        let alt_color = cfg.theme.alt_color();
        let base_url = base_url.clone();

        // capture multiline user input from interactive terminal widget
        let user_query = super::input_widget()
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
            .max_height(super::INPUT_MAX_HEIGHT)
            .multiline(true)
            .render()
            .await?;

        let trimmed = user_query.trim();
        if trimmed.is_empty() {
            continue;
        }

        // evaluate and dispatch slash command instructions
        if trimmed.starts_with('/') {
            let args: Vec<&str> = trimmed.split_whitespace().collect();
            let is_global = args.iter().any(|&a| a == "-g");

            let raw_cmd = args[0].trim_start_matches('/').to_lowercase();
            let clean_args: Vec<&str> = args[1..].iter().copied().filter(|&a| a != "-g").collect();

            // Нормализуем команду/подкоманду и отделяем payload подкоманды от самой подкоманды
            let (cmd, sub_cmd, payload) = match raw_cmd.as_str() {
                "remember" => ("facts".to_string(), "set".to_string(), clean_args.join(" ")),
                "forget" => (
                    "facts".to_string(),
                    "remove".to_string(),
                    clean_args.join(" "),
                ),
                _ => {
                    let sub = clean_args
                        .first()
                        .map(|s| s.to_lowercase())
                        .unwrap_or_default();
                    let p = if clean_args.len() > 1 {
                        clean_args[1..].join(" ")
                    } else {
                        String::new()
                    };
                    (raw_cmd, sub, p)
                }
            };

            // Payload для одиночных команд без подкоманд (например, /rename <name>)
            let top_payload = clean_args.join(" ");

            let sid = session_id.lock().await.clone();

            match cmd.as_str() {
                "exit" | "quit" => return Err("exit".into()),

                "help" => {
                    let cmds = [
                        ("help", "Show this help message"),
                        ("new", "Start a new clear session"),
                        ("clear", "Clear remote chat history"),
                        ("clone", "Clone the current session"),
                        ("rename <name>", "Rename current session"),
                        ("remove", "Remove/delete current session"),
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

                // memory facts
                "facts" | "fact" => match sub_cmd.as_str() {
                    "list" => {
                        let count = payload.parse::<usize>().ok().or(Some(20));
                        let endpoint = format!("{base_url}/users/{}/facts/list", sid.user_id);

                        let content =
                            match send_post(client, &endpoint, Some(&ListQuery { count })).await {
                                Ok(res) => match res.json::<Vec<UserFact>>().await {
                                    Ok(facts) if facts.is_empty() => {
                                        "No stored facts found.".to_string()
                                    }
                                    Ok(facts) => format!(
                                        "Stored facts:\n{}",
                                        facts
                                            .iter()
                                            .map(|f| format!("* [`{}`] {}", f.id, f.text))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    ),
                                    Err(e) => format!("Failed to parse JSON response: {e}"),
                                },
                                Err(err) => err,
                            };

                        render_msg(content).await?;
                        continue;
                    }

                    "set" | "add" | "remember" => {
                        if payload.is_empty() {
                            render_msg("Usage: /facts set <text>").await?;
                            continue;
                        }
                        let endpoint = format!("{base_url}/users/{}/facts/set", sid.user_id);
                        let query = SetQuery {
                            id: None,
                            text: payload.clone(),
                        };

                        let msg = match send_post(client, &endpoint, Some(&query)).await {
                            Ok(res) => match res.json::<UserFact>().await {
                                Ok(fact) => format!(
                                    "Saved to global memory: [{}] {}",
                                    fact.id,
                                    truncate(&fact.text, 40)
                                ),
                                Err(_) => {
                                    format!("Saved to global memory: {}", truncate(&payload, 40))
                                }
                            },
                            Err(err) => err,
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    "remove" | "delete" | "forgot" => {
                        let fact_id: u64 = match payload.trim().parse() {
                            Ok(id) => id,
                            Err(_) => {
                                render_msg("Usage: /facts remove <id> (numeric u64 ID required)")
                                    .await?;
                                continue;
                            }
                        };
                        let endpoint = format!("{base_url}/users/{}/facts/remove", sid.user_id);

                        let msg =
                            match send_post(client, &endpoint, Some(&RemoveQuery { id: fact_id }))
                                .await
                            {
                                Ok(_) => format!("Removed fact `{fact_id}`."),
                                Err(err) => err,
                            };

                        render_msg(msg).await?;
                        continue;
                    }

                    "search" | "find" => {
                        if payload.is_empty() {
                            render_msg("Usage: /facts search <query>").await?;
                            continue;
                        }
                        let endpoint = format!("{base_url}/users/{}/facts/search", sid.user_id);
                        let query = SearchQuery {
                            query: payload,
                            limit: None,
                        };

                        let content = match send_post(client, &endpoint, Some(&query)).await {
                            Ok(res) => {
                                let body = res.text().await.unwrap_or_default();
                                if let Ok(facts) = json::from_str::<Vec<UserFact>>(&body) {
                                    if facts.is_empty() {
                                        "No matching facts found.".to_string()
                                    } else {
                                        format!(
                                            "Found facts:\n{}",
                                            facts
                                                .iter()
                                                .map(|f| format!("* [`{}`]: {}", f.id, f.text))
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        )
                                    }
                                } else if let Ok(facts) = json::from_str::<Vec<String>>(&body) {
                                    if facts.is_empty() {
                                        "No matching facts found.".to_string()
                                    } else {
                                        format!(
                                            "Found facts:\n{}",
                                            facts
                                                .iter()
                                                .map(|f| format!("* [`{f}`]"))
                                                .collect::<Vec<_>>()
                                                .join("\n")
                                        )
                                    }
                                } else {
                                    format!("Failed to parse JSON response: {body}")
                                }
                            }
                            Err(err) => err,
                        };

                        render_msg(content).await?;
                        continue;
                    }

                    "clear" | "clean" => {
                        let endpoint = format!("{base_url}/users/{}/facts/clear", sid.user_id);
                        let msg = match send_post::<()>(client, &endpoint, None).await {
                            Ok(_) => "All global facts cleared.".to_string(),
                            Err(err) => err,
                        };

                        render_msg(msg).await?;
                        continue;
                    }

                    _ => {
                        render_msg(
                            "Unknown facts subcommand. Available: list, add, remove, search, clear",
                        )
                        .await?;
                        continue;
                    }
                },

                // memory rules
                "rules" | "rule" => {
                    let endpoint_prefix = if is_global {
                        format!("{base_url}/users/{}", sid.user_id)
                    } else {
                        format!("{base_url}/sessions/{sid}")
                    };
                    let scope_str = if is_global { "global" } else { "session" };

                    match sub_cmd.as_str() {
                        "list" => {
                            let count = clean_args
                                .get(1)
                                .and_then(|a| a.parse::<usize>().ok())
                                .or(Some(20));
                            let endpoint = format!("{endpoint_prefix}/rules/list");

                            let content = match send_post(
                                client,
                                &endpoint,
                                Some(&ListQuery { count }),
                            )
                            .await
                            {
                                Ok(res) => match res.json::<Vec<UserRule>>().await {
                                    Ok(rules) if rules.is_empty() => {
                                        format!("No active {scope_str} rules found.")
                                    }
                                    Ok(rules) => format!(
                                        "Active {scope_str} rules:\n{}",
                                        rules
                                            .iter()
                                            .map(|r| format!("* [`{}`] {}", r.id, r.text))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                    ),
                                    Err(e) => format!("Failed to parse JSON response: {e}"),
                                },
                                Err(err) => err,
                            };

                            render_msg(content).await?;
                            continue;
                        }

                        "set" | "add" => {
                            if payload.is_empty() {
                                render_msg("Usage: /rules set [-g] <rule text>").await?;
                                continue;
                            }
                            let endpoint = format!("{endpoint_prefix}/rules/set");
                            let query = SetQuery {
                                id: None,
                                text: payload.clone(),
                            };

                            let msg = match send_post(client, &endpoint, Some(&query)).await {
                                Ok(res) => match res.json::<UserRule>().await {
                                    Ok(rule) => format!(
                                        "Applied {scope_str} rule [`{}`]: {}",
                                        rule.id,
                                        truncate(&rule.text, 40)
                                    ),
                                    Err(_) => format!(
                                        "Applied {scope_str} rule: {}",
                                        truncate(&payload, 40)
                                    ),
                                },
                                Err(err) => err,
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        "remove" | "delete" => {
                            let rule_id: u64 = match payload.trim().parse() {
                                Ok(id) => id,
                                Err(_) => {
                                    render_msg("Usage: /rules remove [-g] <id>").await?;
                                    continue;
                                }
                            };
                            let endpoint = format!("{endpoint_prefix}/rules/remove");

                            let msg = match send_post(
                                client,
                                &endpoint,
                                Some(&RemoveQuery { id: rule_id }),
                            )
                            .await
                            {
                                Ok(_) => format!("Removed {scope_str} rule `{rule_id}`."),
                                Err(err) => err,
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        "clear" | "clean" => {
                            let endpoint = format!("{endpoint_prefix}/rules/clear");
                            let msg = match send_post::<()>(client, &endpoint, None).await {
                                Ok(_) => format!("All {scope_str} rules cleared."),
                                Err(err) => err,
                            };

                            render_msg(msg).await?;
                            continue;
                        }

                        _ => {
                            render_msg(
                                "Unknown rules subcommand. Available: list, set, remove, clear",
                            )
                            .await?;
                            continue;
                        }
                    }
                }

                "new" => {
                    let new_sid = SessionId::new(sid.user_id);
                    *session_id.lock().await = new_sid.clone();

                    let endpoint = format!("{base_url}/sessions/{new_sid}/init");
                    let msg = match send_post(
                        client,
                        &endpoint,
                        Some(&super::session_info(new_sid.clone())),
                    )
                    .await
                    {
                        Ok(_) => format!("Started new session: `{new_sid}`."),
                        Err(err) => err,
                    };

                    render_msg(msg).await?;
                    continue;
                }

                "clone" | "duplicate" => {
                    let old_sid = session_id.lock().await.clone();
                    let endpoint = format!("{base_url}/sessions/{old_sid}/clone");

                    let msg = match send_post::<()>(client, &endpoint, None).await {
                        Ok(res) => match res.json::<CloneResponse>().await {
                            Ok(payload) => {
                                let new_sid = payload.id;
                                *session_id.lock().await = new_sid.clone();

                                let _ = send_post::<()>(
                                    client,
                                    &format!("{base_url}/sessions/{old_sid}/finish"),
                                    None,
                                )
                                .await;

                                format!("Cloned current context into new session: `{new_sid}`.")
                            }
                            Err(e) => format!("Failed to parse clone response JSON: {e}"),
                        },
                        Err(err) => err,
                    };

                    render_msg(msg).await?;
                    continue;
                }

                "rename" => {
                    if top_payload.is_empty() {
                        render_msg("Usage: /rename <new_name>").await?;
                        continue;
                    }

                    let endpoint = format!("{base_url}/sessions/{sid}/rename");
                    let query = RenameQuery {
                        name: top_payload.clone(),
                    };

                    let msg = match send_post(client, &endpoint, Some(&query)).await {
                        Ok(_) => format!("Session renamed to `{payload}`."),
                        Err(err) => err,
                    };

                    render_msg(msg).await?;
                    continue;
                }

                "remove" | "delete" => {
                    let endpoint = format!("{base_url}/sessions/{sid}/remove");

                    let msg = match send_post::<()>(client, &endpoint, None).await {
                        Ok(_) => format!("Session `{sid}` removed successfully."),
                        Err(err) => err,
                    };

                    render_msg(msg).await?;
                    return Err("exit".into());
                }

                "clear" | "clean" => {
                    let endpoint = format!("{base_url}/sessions/{sid}/clear");

                    let msg = match send_post::<()>(client, &endpoint, None).await {
                        Ok(_) => "History cleared successfully.".to_string(),
                        Err(err) => err,
                    };

                    render_msg(msg).await?;
                    continue;
                }

                "compact" | "compress" => {
                    let preserve = clean_args
                        .first()
                        .and_then(|i| i.parse::<usize>().ok())
                        .unwrap_or_else(|| Config::get().execution.preserve_messages);

                    super::text_widget("Compressing context...")
                        .title(" Thinking... ".bold().with(brand_color), Align::TopLeft)
                        .spinner_style(SpinnerStyle::Dots)
                        .spinner_color(brand_color)
                        .handler(move |mut ctx| async move {
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
                                                *ctx.state = format!("[{status}]");
                                                ctx.notify();
                                            }
                                            Event::Answer(text) => {
                                                summary.push_str(&text);
                                                *ctx.state = summary.clone();
                                                ctx.notify();
                                            }
                                            Event::Error(err) => {
                                                *ctx.state = format!("Compression error: {err}");
                                                ctx.notify();
                                            }
                                            Event::Dialog(_) => {}
                                            Event::Finish => break,
                                        }
                                    }
                                }
                                Err(e) => {
                                    *ctx.state = format!("Network error: {e}");
                                    ctx.notify();
                                }
                            }
                            ctx.finish();
                        })
                        .render()
                        .await?;
                    continue;
                }

                cmd => {
                    render_msg(format!(
                        "Unknown command `{cmd}`. Print `/help` to see available commands list."
                    ))
                    .await?;
                    continue;
                }
            }
        }

        return Ok(trimmed.to_string());
    }
}
