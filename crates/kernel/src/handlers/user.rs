use crate::prelude::*;
use ovsy_share::{SessionId, UserSessionsQuery};
use tokio::fs;

/// Handles the user sessions list
#[log(skip_all, fields(uid = %uid.0))]
pub async fn handle_list(uid: Paths<u128>, data: Json<UserSessionsQuery>) -> Response {
    let UserSessionsQuery { limit } = data.0;

    match search_sessions(uid.0, limit).await {
        Ok(sessions) => Response::ok().json(&sessions),
        Err(e) => {
            error!("{e}");
            Response::bad_request().text(e.to_string())
        }
    }
}

/// Retrieves the session messages and initializes the session if it doesn't exist
#[log(skip_all)]
async fn search_sessions(user_id: u128, limit: usize) -> Result<Vec<SessionId>> {
    let sessions_dir = path!("$share$/userdata/{user_id}/sessions");

    if !sessions_dir.exists() {
        return Ok(vec![]);
    }

    // open sessions dir:
    let mut entries = match fs::read_dir(&sessions_dir).await {
        Ok(entries) => entries,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            return Ok(Vec::new());
        }
        Err(e) => return Err(e.into()),
    };

    let mut sessions = Vec::new();

    // read all session ids:
    while let Some(entry) = entries.next_entry().await? {
        let file_type = entry.file_type().await?;

        if file_type.is_dir() {
            if let Some(file_name_str) = entry.file_name().to_str() {
                if let Ok(session_id) = file_name_str.parse::<SessionId>() {
                    sessions.push(session_id);
                }
            }
        }
    }

    // sorting by timestamp:
    sessions.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));

    // cut final list by limit:
    if limit > 0 && sessions.len() > limit {
        sessions.truncate(limit);
    }

    Ok(sessions)
}
