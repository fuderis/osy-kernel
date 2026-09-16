use crate::prelude::*;

use anylm::api::{Schema, Tool};
use system_utils::{PowerManager, power::PowerMode};

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //              SCHEDULE POWER
        Tool::new(
            "schedule_power",
            "Schedules or immediately executes a system power action.",
        )
        .required_property(
            "mode",
            Schema::string("Power action to perform.").variants(set![
                "shutdown".into(),
                "reboot".into(),
                "suspend".into(),
                "lock".into(),
                "logout".into(),
            ]),
        )
        .optional_property(
            "timestamp",
            Schema::string(
                "Optional ISO-8601 UTC datetime. If omitted, the action is executed immediately.",
            ),
        ),
        // ________________________________________
        //              CANCEL SCHEDULING
        Tool::new(
            "cancel_power",
            "Cancels the currently scheduled power action if one exists.",
        ),
        // ________________________________________
        //              GET STATUS
        Tool::new(
            "get_power_status",
            "Returns the currently scheduled power action and its execution time, if any.",
        ),
    ]
}

#[derive(Deserialize)]
pub struct PowerAction {
    timestamp: Option<DateTime<Utc>>,
    mode: PowerMode,
}

#[log(skip_all, fields(action))]
pub async fn handle_schedule_power(tx: Sender<Bytes>, action: PowerAction) -> Result<()> {
    let local = action
        .timestamp
        .map(|utc| {
            utc.with_timezone(&Local)
                .format("%A, %I:%M:%S %p (%:z), %B %d, %Y")
                .to_string()
        })
        .unwrap_or("now".into());

    match PowerManager::schedule(action.mode, action.timestamp).await {
        Ok(_) => {
            let msg = format!(
                "Scheduled power action: {mode}. Execution time: {local}.",
                mode = action.mode
            );

            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }

        Err(e) => Err(format!("Power operation failed: {e}").into()),
    }
}

#[log(skip_all)]
pub async fn handle_cancel_power(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    let msg = match PowerManager::cancel().await {
        Some(mode) => format!("Scheduled power action canceled. Canceled action: {mode}."),
        None => "There is no scheduled power action.".into(),
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;
    Ok(())
}

#[log(skip_all)]
pub async fn handle_power_status(tx: Sender<Bytes>, _payload: JsonValue) -> Result<()> {
    let msg = match PowerManager::status().await {
        Some(task) => {
            format!(
                "Scheduled {mode}. Execution time: {local}",
                mode = task.mode,
                local = task
                    .execute_at
                    .with_timezone(&Local)
                    .format("%A, %I:%M:%S %p (%:z), %B %d, %Y"),
            )
        }

        None => "No power action is currently scheduled.".into(),
    };

    info!("{msg}");
    tx.send(Event::Answer(msg))?;
    Ok(())
}
