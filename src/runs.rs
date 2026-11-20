//! Recorded runs of a job: when each started, how it ended, what it reported
//! about itself, and the JSON contract.
use crate::{findings::RunFindings, terminal_text::clean};
use chrono::{DateTime, SecondsFormat, Utc};
use serde::{Deserialize, Deserializer, Serialize, Serializer, de::DeserializeOwned};
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
        #[serde(flatten)]
        report: RunReport,
    },
    /// The recorder disappeared without recording an outcome, as after SIGKILL.
    Lost,
}
impl RunState {
    pub fn failed(&self) -> bool {
        match self {
            Self::Running => false,
            Self::Finished { outcome, .. } => outcome.failed(),
            Self::Lost => true,
        }
    }
    /// The job checked and had nothing to do. Only a clean exit can skip.
    pub fn skipped(&self) -> bool {
        matches!(self, Self::Finished { outcome, report, .. } if report.skipped && outcome.succeeded())
    }
    /// What the run says about whether the job works. Runs still in progress,
    /// runs stopped on request, and skipped runs say nothing.
    pub fn verdict(&self) -> Option<Verdict> {
        match self {
            Self::Running
            | Self::Finished {
                outcome: Outcome::Stopped { .. },
                ..
            } => None,
            _ if self.skipped() => None,
            _ if self.failed() => Some(Verdict::Failed),
            _ => Some(Verdict::Succeeded),
        }
    }
}

/// Whether a job works, according to its newest run that says so.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Verdict {
    Succeeded,
    Failed,
}

/// What a finished run said about itself in its result file. A run without
/// one has the default: not skipped, no summary, no error.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RunReport {
    /// Honored only for a clean exit; see [`RunState::skipped`].
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub skipped: bool,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "summary"
    )]
    pub summary: Option<Summary>,
    /// Why the result file was ignored.
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient"
    )]
    pub result_error: Option<ResultError>,
    /// How the run changed the open problems, when it stated them; see
    /// [`crate::findings::OpenSet::after`].
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "lenient"
    )]
    pub findings: Option<RunFindings>,
}

/// An optional value from another machine, absent when this version does not
/// know it, so a newer Rhythm's additions never make a whole report unreadable.
pub fn lenient<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Option<T>, D::Error> {
    let value = Option::<serde_json::Value>::deserialize(deserializer)?;
    Ok(value.and_then(|v| T::deserialize(v).ok()))
}

/// A list from another machine without the entries this version cannot read.
pub fn lenient_list<'de, D: Deserializer<'de>, T: DeserializeOwned>(
    deserializer: D,
) -> Result<Vec<T>, D::Error> {
    let values = match serde_json::Value::deserialize(deserializer)? {
        serde_json::Value::Array(values) => values,
        _ => vec![],
    };
    Ok(values
        .into_iter()
        .filter_map(|v| T::deserialize(v).ok())
        .collect())
}

/// Every summary, including one from another machine, is cleaned and shortened.
fn summary<'de, D: Deserializer<'de>>(deserializer: D) -> Result<Option<Summary>, D::Error> {
    Ok(Option::<String>::deserialize(deserializer)?.and_then(|text| Summary::new(&text)))
}

/// One line describing a run, safe to print in a terminal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(into = "String")]
pub struct Summary(String);
impl Summary {
    pub const MAX_CHARS: usize = 200;
    /// Cleaned of terminal controls, trimmed, and shortened; `None` when
    /// nothing is left.
    pub fn new(text: &str) -> Option<Self> {
        let text = clean(text.trim());
        if text.is_empty() {
            return None;
        }
        if text.chars().count() <= Self::MAX_CHARS {
            return Some(Self(text));
        }
        let mut short: String = text.chars().take(Self::MAX_CHARS - 1).collect();
        short.push('…');
        Some(Self(short))
    }
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl From<Summary> for String {
    fn from(summary: Summary) -> Self {
        summary.0
    }
}
impl fmt::Display for Summary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a run's result file was ignored. The run is recorded all the same.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ResultError {
    /// Not a regular file, a symlink, or failed to read.
    Unreadable,
    TooLarge,
    /// Not JSON, or not the documented shape.
    Invalid,
    UnsupportedVersion,
}
impl ResultError {
    pub fn code(self) -> &'static str {
        match self {
            Self::Unreadable => "unreadable",
            Self::TooLarge => "too_large",
            Self::Invalid => "invalid",
            Self::UnsupportedVersion => "unsupported_version",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        [
            Self::Unreadable,
            Self::TooLarge,
            Self::Invalid,
            Self::UnsupportedVersion,
        ]
        .into_iter()
        .find(|e| e.code() == code)
    }
}
impl fmt::Display for ResultError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::Unreadable => "unreadable",
            Self::TooLarge => "too large",
            Self::Invalid => "invalid",
            Self::UnsupportedVersion => "unsupported version",
        })
    }
}

/// A job's recorded runs as status reports them.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RecentRuns {
    /// The newest run that was not skipped.
    pub last_run: Option<Run>,
    /// The newest skipped run, when it followed `last_run`.
    pub last_skip: Option<Run>,
    /// What the newest run that says whether the job works said.
    pub verdict: Option<Verdict>,
    /// Problems open as of the newest run that stated them.
    pub open_findings: usize,
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
    /// Ended by a signal after launchd or the user asked the job to stop.
    Stopped {
        signal: i32,
    },
}
impl Outcome {
    pub fn succeeded(self) -> bool {
        self == Self::Code { code: 0 }
    }
    /// A requested stop says nothing about whether the job works.
    pub fn failed(self) -> bool {
        !self.succeeded() && !matches!(self, Self::Stopped { .. })
    }
}
impl fmt::Display for Outcome {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code { code } => write!(f, "exit {code}"),
            Self::Signal { signal } => write!(f, "signal {signal}"),
            Self::SpawnFailed => f.write_str("could not start"),
            Self::Stopped { signal } => write!(f, "stopped (signal {signal})"),
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
                report: RunReport::default(),
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
            (Outcome::Stopped { signal: 15 }, false),
        ] {
            let state = RunState::Finished {
                ended_at: Timestamp(0),
                outcome,
                report: RunReport::default(),
            };
            assert_eq!(state.failed(), failed, "{outcome}");
        }
    }

    fn finished(code: i32, skipped: bool) -> RunState {
        RunState::Finished {
            ended_at: Timestamp(0),
            outcome: Outcome::Code { code },
            report: RunReport {
                skipped,
                ..RunReport::default()
            },
        }
    }

    #[test]
    fn reports_travel_with_finished_runs_and_are_optional() {
        let run = Run {
            id: 8,
            label: "com.example.scan".into(),
            started_at: Timestamp(1_790_000_000_000),
            state: RunState::Finished {
                ended_at: Timestamp(1_790_000_001_000),
                outcome: Outcome::Code { code: 0 },
                report: RunReport {
                    skipped: true,
                    summary: Summary::new("nothing new"),
                    result_error: Some(ResultError::UnsupportedVersion),
                    findings: serde_json::from_value(serde_json::json!({
                        "open": 1,
                        "new": ["rhythm:result"],
                        "resolved": 2,
                        "dropped": 3
                    }))
                    .unwrap(),
                },
            },
        };
        let json = serde_json::to_value(&run).unwrap();
        assert_eq!(json["findings"]["resolved"], 2);
        assert_eq!(json["findings"]["open"], 1);
        assert_eq!(json["skipped"], true);
        assert_eq!(json["summary"], "nothing new");
        assert_eq!(json["result_error"], "unsupported_version");
        assert_eq!(serde_json::from_value::<Run>(json).unwrap(), run);
        // An older Rhythm sends none of them.
        let older: Run = serde_json::from_value(serde_json::json!({
            "id": 8,
            "label": "com.example.scan",
            "started_at": "2026-09-21T14:13:20.000Z",
            "state": "finished",
            "ended_at": "2026-09-21T14:13:21.000Z",
            "outcome": {"type": "code", "code": 0}
        }))
        .unwrap();
        let RunState::Finished { report, .. } = older.state else {
            panic!("{older:?}")
        };
        assert_eq!(report, RunReport::default());
    }

    #[test]
    fn only_a_clean_exit_skips_and_skips_say_nothing_about_the_job() {
        assert!(finished(0, true).skipped());
        assert!(!finished(0, false).skipped());
        // Another machine may claim a skip for a failed run: it still failed.
        assert!(!finished(1, true).skipped());
        assert_eq!(finished(1, true).verdict(), Some(Verdict::Failed));
        assert_eq!(finished(0, true).verdict(), None);
        assert_eq!(finished(0, false).verdict(), Some(Verdict::Succeeded));
        assert_eq!(RunState::Running.verdict(), None);
        assert_eq!(RunState::Lost.verdict(), Some(Verdict::Failed));
        let stopped = RunState::Finished {
            ended_at: Timestamp(0),
            outcome: Outcome::Stopped { signal: 15 },
            report: RunReport::default(),
        };
        assert_eq!(stopped.verdict(), None);
    }

    #[test]
    fn summaries_are_one_clean_line_of_at_most_200_characters() {
        assert_eq!(
            Summary::new("  3 new\titems\n").unwrap().as_str(),
            "3 new?items"
        );
        assert_eq!(Summary::new(" \n "), None);
        assert_eq!(Summary::new("\u{1b}[2J").unwrap().as_str(), "?[2J");
        let exact = "é".repeat(200);
        assert_eq!(Summary::new(&exact).unwrap().as_str(), exact);
        let long = Summary::new(&"é".repeat(201)).unwrap();
        assert_eq!(long.as_str().chars().count(), 200);
        assert!(long.as_str().ends_with("é…"));
        // Text from another machine is cleaned the same way.
        let remote: RunReport =
            serde_json::from_value(serde_json::json!({"summary": "a\u{202E}b"})).unwrap();
        assert_eq!(remote.summary.unwrap().as_str(), "a?b");
        let empty: RunReport = serde_json::from_value(serde_json::json!({"summary": " "})).unwrap();
        assert_eq!(empty.summary, None);
    }

    #[test]
    fn result_errors_have_stable_codes() {
        for error in [
            ResultError::Unreadable,
            ResultError::TooLarge,
            ResultError::Invalid,
            ResultError::UnsupportedVersion,
        ] {
            assert_eq!(ResultError::from_code(error.code()), Some(error));
            assert_eq!(
                serde_json::to_value(error).unwrap(),
                serde_json::json!(error.code())
            );
        }
        assert_eq!(ResultError::from_code("other"), None);
        // A code from a newer Rhythm leaves the rest of the run readable.
        let newer: RunReport = serde_json::from_value(
            serde_json::json!({"skipped": true, "result_error": "from_the_future"}),
        )
        .unwrap();
        assert_eq!((newer.skipped, newer.result_error), (true, None));
    }
}
