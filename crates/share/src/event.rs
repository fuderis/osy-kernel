use crate::{CommandResult, Id};

use anylm::Bytes;
use atoman::{Duration, Result};
use pearce::{Callback, Sender};
use rigging::widgets::Confirmation;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
pub enum Dialog {
    Script {
        id: String,
        code: String,
        #[serde(default)]
        use_sudo: bool,
    },
    Confirm {
        id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<Confirmation>,
    },
    Prompt {
        id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        default: Option<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        multiline: Option<bool>,
    },
    Secret {
        id: String,
        prompt: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Select {
        id: String,
        prompt: String,
        items: Vec<String>,
    },
}

impl Dialog {
    /// Returns dialog event ID.
    pub fn get_id(&self) -> &str {
        match self {
            Self::Confirm { id, .. } => id,
            Self::Prompt { id, .. } => id,
            Self::Secret { id, .. } => id,
            Self::Select { id, .. } => id,
            Self::Script { id, .. } => id,
        }
    }

    /// Universal method for sending any dialogue and waiting for the result.
    pub async fn dispatch<T>(self, tx: &Sender<Bytes>, timeout: Duration) -> Result<Option<T>>
    where
        T: serde::de::DeserializeOwned + Send + 'static,
    {
        let id = self.get_id().to_string();
        let mut callback = Callback::register(&id).await;
        tx.send(Event::Dialog(self))?;

        let output = atoman::select! {
            _ = tx.closed() => return Err(atoman::error::Error::ChannelClosed.into()),
            res = callback.recv::<T>(timeout) => {
                res.map_err(|e| atoman::error::Error::Custom(format!("Dialog action failed: {e}")))?
            }
        };

        Ok(output)
    }

    /// Sends script for execution in CLI (with CommandResult waiting).
    pub async fn script(
        tx: &Sender<Bytes>,
        code: impl Into<String>,
        use_sudo: bool,
    ) -> Result<CommandResult> {
        let id = Id::new().to_string();
        let dialog = Self::Script {
            id,
            code: code.into(),
            use_sudo,
        };

        let res: Option<CommandResult> = dialog.dispatch(tx, Duration::from_secs(300)).await?;
        Ok(res.unwrap_or(CommandResult {
            stdout: String::new(),
            stderr: String::new(),
            exit_code: 0,
            success: true,
        }))
    }

    /// Sends confirmation request [y/n].
    pub async fn confirm(
        tx: &Sender<Bytes>,
        prompt: impl Into<String>,
        default: Option<Confirmation>,
    ) -> Result<Confirmation> {
        let id = Id::new().to_string();
        let dialog = Self::Confirm {
            id,
            prompt: prompt.into(),
            default,
        };

        let res: Option<Confirmation> = dialog.dispatch(tx, Duration::from_secs(120)).await?;
        Ok(res.unwrap_or(Confirmation::No))
    }

    /// Sends text input prompt.
    pub async fn prompt(
        tx: &Sender<Bytes>,
        prompt: impl Into<String>,
        placeholder: Option<String>,
        default: Option<String>,
        multiline: Option<bool>,
    ) -> Result<Option<String>> {
        let id = Id::new().to_string();
        let dialog = Self::Prompt {
            id,
            prompt: prompt.into(),
            placeholder,
            default,
            multiline,
        };

        dialog.dispatch(tx, Duration::from_secs(120)).await
    }

    /// Sends text input prompt of secret data (passwords, tokens).
    pub async fn secret(
        tx: &Sender<Bytes>,
        prompt: impl Into<String>,
        placeholder: Option<String>,
    ) -> Result<Option<String>> {
        let id = Id::new().to_string();
        let dialog = Self::Secret {
            id,
            prompt: prompt.into(),
            placeholder,
        };

        dialog.dispatch(tx, Duration::from_secs(120)).await
    }

    /// Sends drop‑down menu for selecting from a list.
    pub async fn select(
        tx: &Sender<Bytes>,
        prompt: impl Into<String>,
        items: Vec<String>,
    ) -> Result<Option<String>> {
        let id = Id::new().to_string();
        let dialog = Self::Select {
            id,
            prompt: prompt.into(),
            items,
        };

        dialog.dispatch(tx, Duration::from_secs(120)).await
    }
}

/// Assistant stream event
#[derive(Debug, Clone, Serialize, Deserialize, Eq, PartialEq)]
#[serde(tag = "type", content = "content", rename_all = "snake_case")]
pub enum Event {
    Thinking(String),
    Answer(String),
    Error(String),
    Dialog(Dialog),
    Finish,
}

impl Into<String> for Event {
    fn into(self) -> String {
        serde_json::to_string(&self).unwrap()
    }
}

impl Into<Bytes> for Event {
    fn into(self) -> Bytes {
        Into::<String>::into(self).into()
    }
}
