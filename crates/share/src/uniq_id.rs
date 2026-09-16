use macron::Display;
use std::fmt;
use uuid::Uuid;

/// High-performance generator of unique monotonic IDs.
#[derive(Clone, Copy, Display, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Id(Uuid);

impl Id {
    /// Generates new UUIDv7.
    /// Fast-rng (Xoshiro256++), works without slow OS PRNG system calls.
    #[inline]
    pub fn new() -> Self {
        Self(Uuid::now_v7())
    }

    /// Returns string representation without hyphens (32 characters) for compact paths/sockets.
    #[inline]
    pub fn to_short_string(&self) -> String {
        let mut buf = [0u8; 32];
        let hex = self.0.simple().encode_lower(&mut buf);
        hex.to_string()
    }

    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for Id {
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}

impl fmt::Debug for Id {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Id({})", self.0)
    }
}

impl serde::Serialize for Id {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
