//! A launchd job label accepted from operators, agents, and job definitions.
use crate::operation_error::{Error, Result};
use std::{fmt, str::FromStr};

/// Reverse-DNS style: safe as a command argument and a launchctl target.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct Label(String);
impl Label {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for Label {
    type Err = Error;
    fn from_str(value: &str) -> Result<Self> {
        let valid = (1..=255).contains(&value.len())
            && !value.starts_with(['-', '.'])
            && value
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b));
        valid
            .then(|| Self(value.to_owned()))
            .ok_or(Error::InvalidRequest)
    }
}
impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_launchd_style_labels_only() {
        for label in ["com.example.scan", "dev.rhythm.report-1", "a@b_c"] {
            assert!(label.parse::<Label>().is_ok(), "{label}");
        }
        for label in ["", "-x", ".x", "a/b", "a b", "gui/501", "x\n"] {
            assert!(label.parse::<Label>().is_err(), "{label:?}");
        }
        assert!("a".repeat(256).parse::<Label>().is_err());
    }
}
