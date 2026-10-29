//! Recorded runs of a job: when each started, how it ended, and the JSON contract.
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Run {
    pub id: i64,
    pub label: String,
    pub started_at: Timestamp,
    #[serde(flatten)]
    pub state: RunState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum RunState {
    Running,
    Finished {
        ended_at: Timestamp,
        outcome: Outcome,
    },
    /// The recorder disappeared without recording an outcome, as after SIGKILL.
    Lost,
}
impl RunState {
    pub fn failed(&self) -> bool {
        match self {
            Self::Running => false,
            Self::Finished { outcome, .. } => !outcome.succeeded(),
            Self::Lost => true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Outcome {
    Code {
        code: i32,
    },
    Signal {
        signal: i32,
    },
    /// The command could not be started, for example because it does not exist.
    SpawnFailed,
}
impl Outcome {
    pub fn succeeded(self) -> bool {
        self == Self::Code { code: 0 }
    }
}
impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code { code } => write!(f, "exit {code}"),
            Self::Signal { signal } => write!(f, "signal {signal}"),
            Self::SpawnFailed => f.write_str("could not start"),
        }
    }
}

/// Milliseconds since the Unix epoch, exchanged as RFC 3339 UTC.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct Timestamp(pub i64);
impl Timestamp {
    pub fn now() -> Self {
        Self(Utc::now().timestamp_millis())
    }
    pub fn utc(self) -> DateTime<Utc> {
        DateTime::from_timestamp_millis(self.0).unwrap_or_default()
    }
}
impl Serialize for Timestamp {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.utc().to_rfc3339_opts(SecondsFormat::Millis, true))
    }
}
impl<'de> Deserialize<'de> for Timestamp {
    fn deserialize<D: Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let text = String::deserialize(deserializer)?;
        DateTime::parse_from_rfc3339(&text)
            .map(|t| Self(t.timestamp_millis()))
            .map_err(serde::de::Error::custom)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runs_round_trip_through_json() {
        let run = Run {
            id: 7,
            label: "com.example.scan".into(),
            started_at: Timestamp(1_790_000_000_123),
            state: RunState::Finished {
                ended_at: Timestamp(1_790_000_004_000),
                outcome: Outcome::Code { code: 2 },
            },
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(
            json,
            serde_json::json!({
                "id": 7,
                "label": "com.example.scan",
                "started_at": "2026-09-21T14:13:20.123Z",
                "state": "finished",
                "ended_at": "2026-09-21T14:13:24.000Z",
                "outcome": {"type": "code", "code": 2}
            })
        );
        assert_eq!(serde_json::from_value::<Run>(json).unwrap(), run);
    }

    #[test]
    fn only_clean_exits_succeed() {
        assert!(!RunState::Running.failed());
        assert!(RunState::Lost.failed());
        for (outcome, failed) in [
            (Outcome::Code { code: 0 }, false),
            (Outcome::Code { code: 1 }, true),
            (Outcome::Signal { signal: 15 }, true),
            (Outcome::SpawnFailed, true),
        ] {
            let state = RunState::Finished {
                ended_at: Timestamp(0),
                outcome,
            };
            assert_eq!(state.failed(), failed, "{outcome}");
        }
    }
}
