use crate::{prelude::*, utils};

use anylm::api::Message;
use chrono::Local;
use osy_share::{HandleQuery, SessionId};
use rigging::{Stylize, style::Align, widgets::Print};

/// API: Handles interactive chat with assistant.
pub async fn handle_chat(
    uid: u64,
    sid: Option<SessionId>,
    new_session: bool,
    load_history: bool,
    skill_scope: Option<String>,
) -> Result<()> {
    let port = Config::get().server.port;
    let base_url = format!("http://127.0.0.1:{port}");
    let client = Client::tcp();

    // select session or create new
    let sid = if new_session || uid == 0 {
        SessionId::new(uid)
    } else {
        if let Some(session_id) = sid {
            session_id
        } else {
            utils::select_session(&client, &base_url, uid)
                .await?
                .unwrap_or(SessionId::new(uid))
        }
    };
    let session_id = State::from(sid);

    // init session handling
    utils::init_session(&client, &base_url, session_id.get_cloned(), load_history).await?;

    let run_loop = async {
        loop {
            let cfg = Config::get();
            let alt_color = cfg.theme.alt_color();
            let brand_color = cfg.theme.brand_color();

            let trimmed = match utils::render_input(&client, base_url.clone(), &session_id).await {
                Ok(input) => input,
                Err(e) if e.to_string() == "exit" => break,
                Err(e) => return Err(e),
            };

            // prepare query request payload and UI metadata
            let sid = session_id.lock().await.clone();
            let timestamp = Local::now().format("%a %I:%M %p").to_string();
            let user_msg = trimmed.clone();
            let query_msg = Message::user(vec![trimmed.into()]);

            // render user message
            utils::text_widget("")
                .title(format!(" {timestamp} ").with(brand_color), Align::TopLeft)
                .border_color(alt_color)
                .margin_bottom(0)
                .handler(move |mut ctx| async move {
                    *ctx.state = user_msg;
                    ctx.sync();
                    ctx.finish();
                })
                .render()
                .await?;

            // determine query endpoint based on skill_scope
            let query_url = match &skill_scope {
                Some(skill) => format!("{base_url}/sessions/{sid}/skills/{skill}/query"),
                None => format!("{base_url}/sessions/{sid}/query"),
            };

            warn!("{query_url}");

            // render response message
            utils::render_response(
                query_url,
                HandleQuery {
                    current_path: std::env::current_dir().ok(),
                    message: query_msg,
                },
            )
            .await?;
        }

        Ok::<(), DynError>(())
    };

    let res = run_loop.await;

    // flush backend state and finalize active chat session cleanly
    Print::h1("Flushing database and closing session...")
        .render()
        .await?;
    let final_sid = session_id.lock().await.clone();
    let finish_url = format!("http://127.0.0.1:{port}/sessions/{final_sid}/finish");

    if let Err(e) = Client::tcp().post(&finish_url).send().await {
        Print::error(format!("Failed to finish session: {e}"))
            .margin_top(1)
            .render()
            .await?;
    } else {
        Print::success("Session closed successfully.")
            .margin_top(1)
            .render()
            .await?;
    }

    res
}
