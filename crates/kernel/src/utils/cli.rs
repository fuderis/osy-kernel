use crate::prelude::*;

use anylm::api::{Message, Messages, Role, Visibility};
use atoman::Command;
use osy_share::{ListQuery, SessionId, SessionMetadata};
use rigging::{
    Stylize,
    render::Block,
    style::{Align, BorderStyle},
    widgets::{ConfirmPrompt, Input, SelectMenu, Text},
};
use std::process::Stdio;

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
        // attempt to auto-start backend process silently if offline
        if Command::new(path!("$"))
            .args(&["server", "start"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
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

/// Interactive session selection from the list.
pub async fn select_session(
    client: &Client,
    base_url: &str,
    uid: u64,
) -> Result<Option<SessionId>> {
    let sessions_url = format!("{base_url}/users/{uid}/sessions");
    let sessions_query = ListQuery { count: None };

    let response = client
        .post(&sessions_url)
        .json(&sessions_query)
        .send()
        .await?;

    if let Ok(sessions) = response.json::<Vec<SessionMetadata>>().await {
        if !sessions.is_empty() {
            let items: Vec<String> = sessions
                .iter()
                .map(|s| {
                    if let Some(t) = &s.title {
                        format!("{} — {t}", s.session_id)
                    } else {
                        s.session_id.to_string()
                    }
                })
                .collect();

            if let Some(selected_idx) = select_widget("Select a session to connect:", items)
                .render()
                .await?
            {
                if let Some(selected) = sessions.get(selected_idx) {
                    return Ok(Some(selected.session_id));
                }
            }

            // if user canceled (Esc / None) - take most recent session
            return Ok(sessions.first().map(|s| s.session_id));
        }
    }

    Ok(None)
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
        .json(&super::session_info(sid))
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

            // display information message
            let msg_count = valid_messages.len();
            let info_text = format!(
                "Session {} initialized with {} message(s).",
                sid.to_string().with(brand_color),
                msg_count.to_string().with(brand_color)
            );

            text_widget("")
                .handler(move |mut ctx| async move {
                    *ctx.state = info_text;
                    ctx.finish();
                })
                .render()
                .await?;

            if load_history {
                for msg in valid_messages {
                    let is_user = msg.role == Role::User;
                    let border_c = if is_user { alt_color } else { brand_color };
                    let msg_text = msg.extract_texts().concat();
                    let timestamp = if let Some(dt) = msg.timestamp {
                        format!(" {} ", dt.format("%a %I:%M %p").to_string())
                            .with(brand_color)
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
            }
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
        .select_color(brand_color)
        .background_color(bg_color)
        .padding_hor(1)
        .margin_bottom(1)
        .clear_after(true)
}
