use crate::prelude::*;

use osy_share::{
    CompactQuery, Event, ListQuery, RemoveQuery, SearchQuery, SessionId, SetQuery, UserFact,
    UserRule,
};
use rigging::{
    Stylize,
    style::{Align, SpinnerStyle},
};

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

        // --- Phase A: User Input ---
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

        // --- Command Handling ---
        // evaluate and dispatch slash command instructions
        if trimmed.starts_with('/') {
            let args: Vec<&str> = trimmed.split_whitespace().collect();
            let is_global = args.iter().any(|&a| a == "-g");

            // the main command name (e.g., "facts", "rules", "remember")
            let cmd = args[0].trim_start_matches('/').to_lowercase();

            // the second word (action), if passed
            let sub_cmd = args.get(1).map(|s| s.to_lowercase()).unwrap_or_default();

            // pure arguments without a command name, subcommand, or -g flag.
            let clean_args: Vec<&str> = args[1..].iter().copied().filter(|&a| a != "-g").collect();

            // if there is a subcommand (for example, “set” in "/facts set"), the load arguments start from the 2nd element.
            let payload = if !clean_args.is_empty() && clean_args[0].to_lowercase() == sub_cmd {
                clean_args[1..].join(" ")
            } else {
                clean_args.join(" ")
            };

            let sid = session_id.lock().await.clone();

            // helper for formatting long strings (safe for UTF-8 / Cyrillic)
            let truncate = |s: &str, max_len: usize| -> String {
                let char_count = s.chars().count();
                if char_count > max_len {
                    let truncated: String = s.chars().take(max_len).collect();
                    format!("\"{truncated}...\"")
                } else {
                    format!("\"{s}\"")
                }
            };

            // helper for displaying results in the UI
            let render_msg = |msg: String| async move {
                super::text_widget("")
                    .handler(move |mut ctx| async move {
                        *ctx.state = msg;
                        ctx.finish();
                    })
                    .render()
                    .await
            };

            match cmd.as_str() {
                "exit" | "quit" => return Err("exit".into()),

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
                "facts" | "fact" => {
                    match sub_cmd.as_str() {
                        "list" | "ls" => {
                            let count = clean_args
                                .get(1)
                                .and_then(|a| a.parse::<usize>().ok())
                                .or(Some(20));

                            let endpoint = format!("{base_url}/users/{}/facts/list", sid.user_id);
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
                                                    .map(|f| format!("* [`{}`] {}", f.id, f.text))
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
                            let endpoint = format!("{base_url}/users/{}/facts/set", sid.user_id);

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
                                        "Invalid ID `{trimmed_payload}`. Must be a numeric u64 ID."
                                    ))
                                    .await?;
                                    continue;
                                }
                            };

                            let endpoint = format!("{base_url}/users/{}/facts/remove", sid.user_id);

                            let msg = match client
                                .post(&endpoint)
                                .json(&RemoveQuery { id: fact_id })
                                .send()
                                .await
                            {
                                Ok(res) => {
                                    let status = res.status();
                                    if status.is_success() {
                                        format!("Removed fact `{fact_id}`")
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

                            let endpoint = format!("{base_url}/users/{}/facts/search", sid.user_id);
                            let res = client
                                .post(&endpoint)
                                .json(&SearchQuery {
                                    query: payload,
                                    limit: None,
                                })
                                .send()
                                .await;

                            let content = match res {
                                Ok(r) => {
                                    let status = r.status();
                                    if status.is_success() {
                                        match r.text().await {
                                            Ok(body) => {
                                                if let Ok(facts) =
                                                    json::from_str::<Vec<UserFact>>(&body)
                                                {
                                                    if facts.is_empty() {
                                                        "No matching facts found.".to_string()
                                                    } else {
                                                        format!(
                                                            "Found facts:\n{}",
                                                            facts
                                                                .iter()
                                                                .map(|f| format!(
                                                                    "* [`{}`]: {}",
                                                                    f.id, f.text
                                                                ))
                                                                .collect::<Vec<_>>()
                                                                .join("\n")
                                                        )
                                                    }
                                                } else if let Ok(facts) =
                                                    json::from_str::<Vec<String>>(&body)
                                                {
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
                            let endpoint = format!("{base_url}/users/{}/facts/clear", sid.user_id);

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
                    }
                }

                // --- Memory: Rules ---
                "rules" | "rule" => match sub_cmd.as_str() {
                    "list" | "ls" => {
                        let count = clean_args
                            .get(1)
                            .and_then(|a| a.parse::<usize>().ok())
                            .or(Some(20));

                        let endpoint = if is_global {
                            format!("{base_url}/users/{}/rules/list", sid.user_id)
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
                                                    "* [`{}`] {}",
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
                            format!("{base_url}/users/{}/rules/set", sid.user_id)
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
                            format!("{base_url}/users/{}/rules/remove", sid.user_id)
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
                            format!("{base_url}/users/{}/rules/clear", sid.user_id)
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
                            "Unknown rules subcommand. Available: list, set, remove, clear".into(),
                        )
                        .await?;
                        continue;
                    }
                },

                "new" => {
                    let new_sid = SessionId::new(sid.user_id);
                    *session_id.lock().await = new_sid.clone();

                    let msg = match client
                        .post(&format!("{base_url}/sessions/{new_sid}/init"))
                        .json(&super::session_info(new_sid))
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
                    // obtain the ID of the current (original) session.
                    let old_sid = session_id.lock().await.clone();

                    #[derive(serde::Deserialize)]
                    struct CloneResponse {
                        id: SessionId,
                    }

                    // send a POST request for cloning.
                    let msg = match client
                        .post(&format!("{base_url}/sessions/{old_sid}/clone"))
                        .send()
                        .await
                    {
                        Ok(res) => {
                            let status = res.status();
                            if status.is_success() {
                                // read the JSON with the new ID from the server response.
                                match res.json::<CloneResponse>().await {
                                    Ok(payload) => {
                                        let new_sid = payload.id;

                                        // updating the local session ID
                                        *session_id.lock().await = new_sid.clone();

                                        // sending a signal to close the previous session
                                        client
                                            .post(&format!("{base_url}/sessions/{old_sid}/finish"))
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
