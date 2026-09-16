use crate::prelude::*;

use anylm::api::{Schema, Tool};
use system_utils::{SystemTheme, ThemeStyle};

pub fn tools_list() -> Vec<Tool> {
    vec![
        // ________________________________________
        //              SET THEME
        Tool::new("set_theme", "Changes the system appearance theme.").required_property(
            "style",
            Schema::string("Target theme style.").variants(set!["light".into(), "dark".into()]),
        ),
    ]
}

#[derive(Deserialize)]
pub struct ThemeAction {
    style: ThemeStyle,
}

#[log(skip_all, fields(action))]
pub async fn handle_set_theme(tx: Sender<Bytes>, action: ThemeAction) -> Result<()> {
    match SystemTheme::switch(action.style.clone()).await {
        Ok(_) => {
            let msg = format!("System theme switched into {} mode", action.style);
            info!("{msg}");
            tx.send(Event::Answer(msg))?;
            Ok(())
        }
        Err(e) => Err(format!("Switching system theme failed: {e}").into()),
    }
}

#[log(skip_all, fields(action))]
pub async fn handle_get_theme(_tx: Sender<Bytes>, _action: ()) -> Result<()> {
    Err(Error::Custom(str!("Action `get system theme` is not implemented yet.")).into())

    // TODO: write get_system_theme tool
}
