use crate::prelude::*;

#[derive(Deserialize, Schema)]
pub struct HelloAction {
    pub name: Option<String>,
}

#[log()]
pub async fn handle_hello(tx: Sender<Bytes>, query: ToolQuery<HelloAction>) -> Result<()> {
    let ToolQuery { payload, .. } = query;

    let target = payload.name.unwrap_or("World".into());
    tx.send(Event::Answer(format!("Hello, {target}!")))?;

    Ok(())
}
