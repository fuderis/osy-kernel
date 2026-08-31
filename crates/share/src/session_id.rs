use chrono::{DateTime, TimeZone, Utc};
use macron::{Display, Error};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use sha2::{Digest, Sha512_256};
use std::{num::ParseIntError, str::FromStr};

/// The session ID wrapper
#[derive(Default, Debug, Display, Copy, Clone, Eq, PartialEq, Hash)]
#[display(fmt = "{user_id}-{timestamp}-{salt}")]
pub struct SessionId {
    pub user_id: u64,
    pub timestamp: u128,
    pub salt: u16,
}

impl SessionId {
    /// Creates a new session ID
    pub fn new(user_id: u64) -> Self {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("Time went backwards")
            .as_millis();

        Self {
            timestamp,
            user_id,
            salt: rand::random(),
        }
    }

    /// Get the session creation time
    pub fn created_at(&self) -> DateTime<Utc> {
        let secs = (self.timestamp / 1_000) as i64;
        let nsecs = ((self.timestamp % 1_000) * 1_000_000) as u32;

        Utc.timestamp_opt(secs, nsecs)
            .single()
            .unwrap_or_else(Utc::now)
    }

    /// Generates a cryptographically secure SHA-256 hash
    pub fn to_hash(&self) -> String {
        let hash = Sha512_256::digest(self.to_string());
        hash.iter().map(|b| format!("{:02x}", b)).collect()
    }
}

impl FromStr for SessionId {
    type Err = SessionIdError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('-');

        let user_id_str = parts.next().ok_or(SessionIdError::InvalidFormat)?;
        let timestamp_str = parts.next().ok_or(SessionIdError::InvalidFormat)?;
        let salt_str = parts.next().ok_or(SessionIdError::InvalidFormat)?;

        if parts.next().is_some() {
            return Err(SessionIdError::InvalidFormat);
        }

        let user_id = user_id_str
            .parse::<u64>()
            .map_err(SessionIdError::InvalidUserId)?;
        let timestamp = timestamp_str
            .parse::<u128>()
            .map_err(SessionIdError::InvalidTimestamp)?;
        let salt = salt_str
            .parse::<u16>()
            .map_err(SessionIdError::InvalidSalt)?;

        Ok(Self {
            user_id,
            timestamp,
            salt,
        })
    }
}

impl Serialize for SessionId {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}

impl<'de> Deserialize<'de> for SessionId {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        FromStr::from_str(&s).map_err(serde::de::Error::custom)
    }
}

/// The session id parsing error
#[derive(Debug, Display, Error)]
pub enum SessionIdError {
    InvalidFormat,
    InvalidUserId(ParseIntError),
    InvalidTimestamp(ParseIntError),
    InvalidSalt(ParseIntError),
}
