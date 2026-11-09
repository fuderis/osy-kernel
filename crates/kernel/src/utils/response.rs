use crate::prelude::*;

use atoman::io::AsyncWriteExt;
use osy_share::{CommandResult, Dialog, Event};
use rigging::{
    Stylize,
    render::SubWidgetPosition,
    style::{Align, LineStyle, SpinnerStyle},
};
use std::process::Stdio;

/// Renders CLI response widget.
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

    super::text_widget("")
        .title(format!(" {timestamp} ").with(brand_color), Align::TopLeft)
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
                    let mut received_any_event = false;

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
                        received_any_event = true;

                        match event {
                            Event::Thinking(status) => {
                                status_msg = Some(format!("{}", status.italic().dim()));
                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                ctx.sync();
                                ctx.notify();
                            }
                            Event::Answer(chunk) => {
                                full_response.push_str(&chunk);
                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                ctx.sync();
                                ctx.notify();
                            }
                            Event::Error(err) => {
                                let formatted_err = if let Some((title, msg)) = err.split_once(':')
                                {
                                    format!(
                                        "{} {} {msg}",
                                        "Error:".red().bold(),
                                        title.trim().red().bold(),
                                    )
                                } else {
                                    format!("{} {}", "Error:".red().bold(), err.trim())
                                };

                                status_msg = Some(formatted_err);
                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                ctx.sync();
                                ctx.notify();
                            }

                            // --- Interactive Dialog Event Handling ---
                            Event::Dialog(dialog_event) => match dialog_event {
                                // bash execution in background with explicit sudo check
                                Dialog::Script { id, code, use_sudo } => {
                                    #[cfg(target_os = "linux")]
                                    {
                                        // if sudo is required, check ‘sudo -n true’ with suppressed output
                                        let sudo_valid = if use_sudo {
                                            atoman::process::Command::new("sudo")
                                                .arg("-n")
                                                .arg("true")
                                                .stdout(Stdio::null())
                                                .stderr(Stdio::null())
                                                .status()
                                                .await
                                                .map(|s| s.success())
                                                .unwrap_or(false)
                                        } else {
                                            true
                                        };

                                        // if sudo is needed but there is no valid session, request the password via the TUI widget
                                        if use_sudo && !sudo_valid {
                                            let input_block = super::input_widget()
                                                .title(
                                                    " [sudo] password required "
                                                        .bold()
                                                        .with(brand_color),
                                                    Align::TopLeft,
                                                )
                                                .margin_bottom(1)
                                                .secret(true);

                                            if let Ok(password) = ctx
                                                .show_sub_widget(
                                                    input_block,
                                                    SubWidgetPosition::Replace,
                                                )
                                                .await
                                            {
                                                if !password.trim().is_empty() {
                                                    // validate and cache the session using sudo -S -v.
                                                    if let Ok(mut child) =
                                                        atoman::process::Command::new("sudo")
                                                            .arg("-S")
                                                            .arg("-v")
                                                            .stdin(Stdio::piped())
                                                            .stdout(Stdio::null())
                                                            .stderr(Stdio::null())
                                                            .spawn()
                                                    {
                                                        if let Some(mut stdin) = child.stdin.take()
                                                        {
                                                            let _ = stdin
                                                                .write_all(password.as_bytes())
                                                                .await;
                                                        }
                                                        let _ = child.wait().await;
                                                    }
                                                }
                                            }
                                        }
                                    }

                                    let running_status = format!(
                                        "{}\n{}",
                                        "Executing script:".yellow().bold(),
                                        code.lines()
                                            .map(|l| format!("  {l}"))
                                            .collect::<Vec<_>>()
                                            .join("\n")
                                            .dim()
                                    );

                                    status_msg = Some(running_status);

                                    update_ui(
                                        &mut ctx.state,
                                        &full_response,
                                        status_msg.as_deref(),
                                    );
                                    ctx.sync();
                                    ctx.notify();

                                    #[cfg(target_os = "windows")]
                                    let output = atoman::process::Command::new("cmd")
                                        .arg("/C")
                                        .arg(&code)
                                        .output()
                                        .await;

                                    #[cfg(not(target_os = "windows"))]
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
                                    ctx.sync();
                                    ctx.notify();
                                }

                                // action confirmation [y/n]
                                Dialog::Confirm {
                                    id,
                                    prompt,
                                    default,
                                } => {
                                    let mut confirm_block = super::confirm_widget(&prompt);
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
                                Dialog::Prompt {
                                    id,
                                    prompt,
                                    placeholder,
                                    default,
                                    multiline,
                                } => {
                                    let is_multi = multiline.unwrap_or(false);
                                    let mut input_block = super::input_widget()
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
                                Dialog::Secret {
                                    id,
                                    prompt,
                                    placeholder,
                                } => {
                                    let mut input_block = super::input_widget()
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
                                Dialog::Select { id, prompt, items } => {
                                    let select_block = super::select_widget(prompt, items);

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
                                if !full_response.trim().is_empty() {
                                    let _ = status_msg.take();
                                }

                                update_ui(&mut ctx.state, &full_response, status_msg.as_deref());
                                break;
                            }
                        }
                    }

                    if !received_any_event && full_response.is_empty() {
                        *ctx.state = format!(
                            "{} Server returned empty response (Check status/URL: {url})",
                            "Error:".red().bold()
                        );
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
