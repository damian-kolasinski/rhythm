use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(try_from = "String", into = "String")]
pub struct WorkloadId(String);
impl std::str::FromStr for WorkloadId {
    type Err = &'static str;
    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::try_from(value.to_owned())
    }
}

impl TryFrom<String> for WorkloadId {
    type Error = &'static str;
    fn try_from(value: String) -> Result<Self, Self::Error> {
        if value.is_empty()
            || value.len() > 63
            || !value.as_bytes()[0].is_ascii_lowercase()
            || !value
                .bytes()
                .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == b'-' || c == b'_')
        {
            return Err("invalid_workload_id");
        }
        Ok(Self(value))
    }
}
impl From<WorkloadId> for String {
    fn from(id: WorkloadId) -> String {
        id.0
    }
}
impl fmt::Display for WorkloadId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        self.0.fmt(f)
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn workload_identifiers_have_bounded_path_safe_ascii_names() {
        assert!(WorkloadId::try_from("a".repeat(63)).is_ok());
        assert!(WorkloadId::try_from("sample-worker_2".to_owned()).is_ok());
        for value in [
            "".into(),
            "a".repeat(64),
            "../sample".into(),
            "Uppercase".into(),
            "3sample".into(),
            "sample name".into(),
            "é".into(),
        ] {
            assert!(WorkloadId::try_from(value).is_err());
        }
    }
}
