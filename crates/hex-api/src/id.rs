use serde::{Deserialize, Serialize};

use crate::Refusal;

/// A box's name on its node. It names the box's cgroup and its state directory, so it is never a
/// path: 1 to [`BoxId::MAX_LEN`] characters of `A-Z a-z 0-9 _ + - .`, not starting with `.`. That is
/// the set runc accepts for a container id, so an OCI runtime's caller can use its own ids.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct BoxId(String);

impl BoxId {
    /// Longest id: well under a cgroup or file name's 255 bytes, with room for the suffixes Hex adds.
    pub const MAX_LEN: usize = 128;

    /// `s` as an id, or why it can't be one.
    pub fn new(s: &str) -> Result<Self, Refusal> {
        let invalid = |why: &str| Refusal::Invalid {
            what: "box id",
            why: format!("{s:?}: {why}"),
        };
        if s.is_empty() {
            return Err(invalid("empty"));
        }
        if s.len() > Self::MAX_LEN {
            return Err(invalid("longer than 128 characters"));
        }
        if s.starts_with('.') {
            return Err(invalid("starts with '.'"));
        }
        if let Some(c) = s
            .chars()
            .find(|c| !(c.is_ascii_alphanumeric() || matches!(c, '_' | '+' | '-' | '.')))
        {
            return Err(invalid(&format!("holds {c:?}")));
        }
        Ok(Self(s.to_owned()))
    }

    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for BoxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl TryFrom<String> for BoxId {
    type Error = Refusal;
    fn try_from(s: String) -> Result<Self, Refusal> {
        Self::new(&s)
    }
}

impl From<BoxId> for String {
    fn from(id: BoxId) -> String {
        id.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_id_is_never_a_path_or_empty() {
        for bad in ["", ".", "..", ".hidden", "a/b", "a\0b", "über", "a b"] {
            assert!(BoxId::new(bad).is_err(), "{bad:?} accepted");
        }
        assert!(BoxId::new(&"x".repeat(129)).is_err());
        for good in [
            "a",
            "leg-1.2_3+x",
            "0f8e2c1a-55a2-4d0e-9f43-3b1c2d4e5f60",
            &"x".repeat(128),
        ] {
            assert_eq!(BoxId::new(good).unwrap().as_str(), good);
        }
    }

    #[test]
    fn a_decoded_id_is_checked_like_a_made_one() {
        assert!(serde_json::from_str::<BoxId>("\"../etc\"").is_err());
        let id: BoxId = serde_json::from_str("\"ok-1\"").unwrap();
        assert_eq!(id.as_str(), "ok-1");
    }
}
