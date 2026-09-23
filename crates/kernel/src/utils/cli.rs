use crate::prelude::*;

use anylm::api::{Message, Messages, Role, Visibility};
use atoman::Command;
use osy_share::{CommandResult, DialogEvent, Event, ListQuery};
use rigging::{
    Stylize,
    render::Block,
    render::SubWidgetPosition,
    style::{Align, BorderStyle, LineStyle, SpinnerStyle},
    widgets::{ConfirmPrompt, Input, SelectMenu, Text},
};

pub const MIN_WIDTH: usize = 80;
pub const INPUT_MAX_HEIGHT: usize = 20;

/// Ensures that kernel server is started.
pub async fn ensure_server(client: &Client, base_url: &str) -> Result<()> {
    // verify whether the backend server is reachable
    if client
        .get(&format!("{base_url}/ping"))
        .send()
        .await
        .is_err()
    {
        // attempt to auto-start backend process if offline
        if Command::new(path!("$"))
            .args(&["server", "start"])
            .spawn()
            .is_ok()
        {
            let ping_url = format!("{base_url}/ping");
            let mut is_ok = false;

            // poll status endpoint until service responds or times out
            for _ in 0..10 {
                atoman::time::sleep(Duration::from_millis(1000)).await;
                if client
                    .get(&ping_url)
                    .timeout(Duration::from_millis(1000))
                    .send()
                    .await
                    .is_ok()
                {
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

    Ok(())
}

/// Creates stylized Text widget.
pub fn text_widget(content: impl Into<String>) -> Block<Text> {
    let cfg = Config::get();
    let brand_color = cfg.theme.brand_color();
    let bg_color = cfg.theme.bg_color();

    Text::new(content)
        .markdown(true)
        .min_width(MIN_WIDTH)
        .border_style(BorderStyle::Rounded)
        .border_color(brand_color)
        .accent_color(brand_color)
        .background_color(bg_color)
        .padding_hor(1)
        .margin_bottom(1)
}

/// Creates stylized Input widget.
pub fn input_widget() -> Block<Input> {
    let cfg = Config::get();
    let brand_color = cfg.theme.brand_color();
    let bg_color = cfg.theme.bg_color();

    Input::new()
        .min_width(MIN_WIDTH)
        .border_style(BorderStyle::Rounded)
        .border_color(brand_color)
        .background_color(bg_color)
        .padding_hor(1)
        .show_cursor(true)
        .clear_after(true)
}

/// Creates stylized ConfirmPrompt widget.
pub fn confirm_widget(prompt: impl AsRef<str>) -> Block<ConfirmPrompt> {
    let cfg = Config::get();
    let brand_color = cfg.theme.brand_color();
    let bg_color = cfg.theme.bg_color();

    ConfirmPrompt::new(prompt.as_ref())
        .markdown(true)
        .min_width(MIN_WIDTH)
        .border_style(BorderStyle::Rounded)
        .border_color(brand_color)
        .accent_color(brand_color)
        .background_color(bg_color)
        .padding_hor(1)
        .margin_bottom(1)
        .clear_after(true)
}

/// Creates stylized SelectMenu widget.
pub fn select_widget(prompt: impl Into<String>, items: Vec<String>) -> Block<SelectMenu> {
    let cfg = Config::get();
    let brand_color = cfg.theme.brand_color();
    let bg_color = cfg.theme.bg_color();

    SelectMenu::new(prompt, items)
        .min_width(MIN_WIDTH)
        .border_style(BorderStyle::Rounded)
        .border_color(brand_color)
        .background_color(bg_color)
        .padding_hor(1)
        .margin_bottom(1)
        .clear_after(true)
}

/// Initializes user session.
pub async fn init_session(
    client: &Client,
    base_url: &str,
    sid: SessionId,
    load_history: bool,
) -> Result<()> {
    // initialize session on backend and retrieve history payload
    let init_res = client
        .post(&format!("{base_url}/sessions/{sid}/init"))
        .json(&super::session_info())
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
            let cfg = Config::get();
            let brand_color = cfg.theme.brand_color();
            let alt_color = cfg.theme.alt_color();

            if load_history {
                for msg in valid_messages {
                    let is_user = msg.role == Role::User;
                    let border_c = if is_user { alt_color } else { brand_color };
                    let msg_text = msg.extract_texts().concat();
                    let timestamp = if let Some(dt) = msg.timestamp {
                        format!(" {} ", dt.format("%a %I:%M %p").to_string())
                            .with(alt_color)
                            .to_string()
                    } else {
                        str!(" -:- ")
                    };

                    text_widget("")
                        .title(timestamp, Align::TopLeft)
                        .border_color(border_c)
                        .margin_bottom(if is_user { 0 } else { 1 })
                        .handler(move |mut ctx| async move {
                            *ctx.state = msg_text;
                            ctx.finish();
                        })
                        .render()
                        .await?;
                }
            } else {
                let dialog_pairs = valid_messages.len() / 2;
                if dialog_pairs > 0 {
                    let info_text = format!(
                        "Loaded {} previous dialog(s) in background.",
                        dialog_pairs.to_string().with(brand_color)
                    );

                    text_widget("")
                        .handler(move |mut ctx| async move {
                            *ctx.state = info_text;
                            ctx.notify();
                            ctx.finish();
                        })
                        .render()
                        .await?;
                }
            }
        }
    }

    Ok(())
}

/// Returns recently active session ID.
pub async fn get_last_sid(client: &Client, base_url: &str, uid: u64) -> SessionId {
    let sessions_url = format!("{base_url}/users/{uid}/sessions");
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
                return last_session;
            }
        }
    }

    SessionId::new(uid)
}

pub async fn render_response<T>(url: String, body: T) -> Result<()>
where
    T: Serialize + Send + 'static,
{
    let cfg = Config::get();
    let port = cfg.server.port;
    let base_url = format!("http://127.0.0.1:{port}");
    let brand_color = cfg.theme.brand_color();
    let alt_color = cfg.theme.alt_color();
    let blink_color = cfg.theme.blink_color();

    let timestamp = Local::now().format("%a %I:%M %p").to_string();

    text_widget("")
        .title(format!(" {timestamp} ").with(alt_color), Align::TopLeft)
        .spinner_style(SpinnerStyle::MiniDots)
        .spinner_color(brand_color)
        .prefix_line(LineStyle::Solid)
        .prefix_color(alt_color)
        .blink_color(blink_color)
        .handler(move |mut ctx| async move {
            let mut full_response = String::new();
            let mut status_msg: Option<String> = None;

            // establish async SSE event stream connection with server
            let res = Client::tcp().post(&url).json(&body).stream::<Event>().await;

            match res {
                Ok(mut stream) => {
                    let update_ui = |state: &mut String, resp: &str, status: Option<&str>| match (
                        resp.is_empty(),
                        status,
                    ) {
                        (true, Some(st)) => *state = st.to_string(),
                        (false, Some(st)) => *state = format!("{resp}\n\n{st}"),
                        (_, None) => *state = resp.to_string(),
                    };

                    // process streaming tokens and update widget buffer in real time
                    while let Ok(Some(event)) = stream.recv().await {
                        match event {
                            Event::Thinking(status) => {
                                status_msg = Some(format!("{}", status.italic().dim()));
                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                ctx.sync_n(3);
                                ctx.notify();
                            }
                            Event::Answer(chunk) => {
                                full_response.push_str(&chunk);
                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                ctx.sync_n(3);
                                ctx.notify();
                            }
                            Event::Error(err) => {
                                let formatted_err = if let Some((title, msg)) = err.split_once(':')
                                {
                                    format!(
                                        "{}{} {msg}",
                                        "Error: ".red().bold(),
                                        title.trim().red().bold(),
                                    )
                                } else {
                                    format!("{} {}", "Error: ".red().bold(), err.trim())
                                };

                                status_msg = Some(formatted_err);
                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                ctx.sync_n(3);
                                ctx.notify();
                            }

                            // --- Interactive Dialog Event Handling ---
                            Event::Dialog(dialog_event) => match dialog_event {
                                // bash execution in background
                                DialogEvent::Script { id, code } => {
                                    status_msg =
                                        Some(format!("{}", "Executing script...".italic().dim()));
                                    update_ui(
                                        &mut ctx.state,
                                        &full_response,
                                        status_msg.as_deref(),
                                    );
                                    ctx.sync_n(3);
                                    ctx.notify();

                                    let output = atoman::process::Command::new("bash")
                                        .arg("-c")
                                        .arg(&code)
                                        .output()
                                        .await;

                                    let result_payload = match output {
                                        Ok(out) => CommandResult {
                                            stdout: String::from_utf8_lossy(&out.stdout)
                                                .to_string(),
                                            stderr: String::from_utf8_lossy(&out.stderr)
                                                .to_string(),
                                            exit_code: out.status.code().unwrap_or(-1) as i16,
                                            success: out.status.success(),
                                        },
                                        Err(e) => CommandResult {
                                            stdout: String::new(),
                                            stderr: e.to_string(),
                                            exit_code: -1,
                                            success: false,
                                        },
                                    };

                                    let callback_url = format!("{base_url}/callback/{id}");
                                    let _ = Client::tcp()
                                        .post(&callback_url)
                                        .json(&result_payload)
                                        .send()
                                        .await;

                                    status_msg = None;
                                    update_ui(
                                        &mut ctx.state,
                                        &full_response,
                                        status_msg.as_deref(),
                                    );
                                    ctx.sync_n(3);
                                    ctx.notify();
                                }

                                // action confirmation [y/n]
                                DialogEvent::Confirm {
                                    id,
                                    prompt,
                                    default,
                                } => {
                                    let mut confirm_block = confirm_widget(&prompt);
                                    if let Some(def) = default {
                                        confirm_block = confirm_block.default(def);
                                    }

                                    if let Ok(value) = ctx
                                        .show_sub_widget(confirm_block, SubWidgetPosition::Replace)
                                        .await
                                    {
                                        let callback_url = format!("{base_url}/callback/{id}");
                                        let _ = Client::tcp()
                                            .post(&callback_url)
                                            .json(&value)
                                            .send()
                                            .await;
                                    }
                                }

                                // prompt text input
                                DialogEvent::Prompt {
                                    id,
                                    prompt,
                                    placeholder,
                                    default,
                                    multiline,
                                } => {
                                    let is_multi = multiline.unwrap_or(false);
                                    let mut input_block = input_widget()
                                        .title(prompt.bold().with(brand_color), Align::TopLeft)
                                        .margin_bottom(1)
                                        .multiline(is_multi);

                                    if let Some(ph) = placeholder {
                                        input_block = input_block.placeholder(ph.with(alt_color));
                                    }
                                    if let Some(def) = default {
                                        input_block = input_block.default_val(def);
                                    }

                                    if let Ok(res) = ctx
                                        .show_sub_widget(input_block, SubWidgetPosition::Replace)
                                        .await
                                    {
                                        let value = if res.trim().is_empty() {
                                            None
                                        } else {
                                            Some(res)
                                        };
                                        let callback_url = format!("{base_url}/callback/{id}");
                                        let _ = Client::tcp()
                                            .post(&callback_url)
                                            .json(&value)
                                            .send()
                                            .await;
                                    }
                                }

                                // secret input
                                DialogEvent::Secret {
                                    id,
                                    prompt,
                                    placeholder,
                                } => {
                                    let mut input_block = input_widget()
                                        .title(prompt.bold().with(brand_color), Align::TopLeft)
                                        .margin_bottom(1)
                                        .secret(true);

                                    if let Some(ph) = placeholder {
                                        input_block = input_block.placeholder(ph.with(alt_color));
                                    }

                                    if let Ok(res) = ctx
                                        .show_sub_widget(input_block, SubWidgetPosition::Replace)
                                        .await
                                    {
                                        let value = if res.trim().is_empty() {
                                            None
                                        } else {
                                            Some(res)
                                        };
                                        let callback_url = format!("{base_url}/callback/{id}");
                                        let _ = Client::tcp()
                                            .post(&callback_url)
                                            .json(&value)
                                            .send()
                                            .await;
                                    }
                                }

                                // select menu
                                DialogEvent::Select { id, prompt, items } => {
                                    let select_block = select_widget(prompt, items);

                                    if let Ok(value) = ctx
                                        .show_sub_widget(select_block, SubWidgetPosition::Replace)
                                        .await
                                    {
                                        let callback_url = format!("{base_url}/callback/{id}");
                                        let _ = Client::tcp()
                                            .post(&callback_url)
                                            .json(&value)
                                            .send()
                                            .await;
                                    }
                                }
                            },

                            Event::Finish => {
                                let _ = status_msg.take();
                                update_ui(&mut ctx.state, &full_response, None);
                                break;
                            }
                        }
                    }

                    ctx.sync();
                    ctx.notify();
                }
                Err(err) => {
                    *ctx.state = format!("\n{} Connection failed: {err}", "Error:".red().bold());
                    ctx.notify();
                }
            }
            ctx.finish();
        })
        .render()
        .await?;

    Ok(())
}
