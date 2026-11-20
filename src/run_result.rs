//! The result file a job may write through `RHYTHM_RESULT` to say how its run
//! went: where it lives, and how it is read and validated. Writing one is
//! optional; without it a run is recorded exactly as before.
use crate::{
    findings::{self, Reported, Said},
    runs::{ResultError, RunReport, Summary},
};
use serde::Deserialize;
use std::{
    fs::File,
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

/// Names the result file in the command's environment.
pub const VARIABLE: &str = "RHYTHM_RESULT";
const FILE: &str = "result.json";
pub const MAX_BYTES: u64 = 256 * 1024;
const VERSION: u64 = 1;

/// Where the run whose private output directory is `dir` may write its result.
pub fn path(dir: &Path) -> PathBuf {
    dir.join(FILE)
}

/// What a job said about its run, as written.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct JobResult {
    pub skipped: bool,
    pub summary: Option<Summary>,
    /// The problems open now, when the job listed them.
    pub findings: Option<Reported>,
}
impl JobResult {
    /// Findings count whatever the outcome: a job can fail at one part of
    /// its work and still know what is open.
    fn collected(self) -> Collected {
        Collected {
            report: RunReport {
                skipped: self.skipped,
                summary: self.summary,
                ..RunReport::default()
            },
            findings: Said::Readable(self.findings),
        }
    }
}

/// What a run's result file said: the run's report, still without the open
/// findings that follow from the previous run's, and what it said about them.
#[derive(Debug, PartialEq, Eq)]
pub struct Collected {
    pub report: RunReport,
    pub findings: Said,
}
impl Collected {
    /// A run that wrote no result.
    pub fn nothing() -> Self {
        Self {
            report: RunReport::default(),
            findings: Said::Nothing,
        }
    }
}

/// What the run whose output directory is `dir` wrote to its result file.
/// Never fails: a bad file becomes the run's `result_error`. Whether a
/// reported skip counts depends on how the run ended; see
/// [`RunState::skipped`](crate::runs::RunState::skipped).
pub fn collect(dir: &Path) -> Collected {
    match read(dir) {
        Ok(None) => Collected::nothing(),
        Ok(Some(result)) => result.collected(),
        Err(error) => Collected {
            report: RunReport {
                result_error: Some(error),
                ..RunReport::default()
            },
            findings: Said::Unreadable(error),
        },
    }
}

/// Reads only a regular file, never through a symlink, and never blocks on
/// a FIFO. `None` when the job wrote nothing.
pub fn read(dir: &Path) -> Result<Option<JobResult>, ResultError> {
    let file = match File::options()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path(dir))
    {
        Ok(file) => file,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(ResultError::Unreadable),
    };
    let metadata = file.metadata().map_err(|_| ResultError::Unreadable)?;
    if !metadata.is_file() {
        return Err(ResultError::Unreadable);
    }
    if metadata.len() > MAX_BYTES {
        return Err(ResultError::TooLarge);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ResultError::Unreadable)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(ResultError::TooLarge);
    }
    parse(&bytes).map(Some)
}

/// Validates version 1: `{"version": 1, "skipped": bool, "summary": string,
/// "findings": [...]}`, where only `version` is required and unknown fields
/// are ignored so later additions to version 1 stay readable. See
/// [`findings::parse`] for the findings.
pub fn parse(bytes: &[u8]) -> Result<JobResult, ResultError> {
    #[derive(Deserialize)]
    struct Versioned {
        version: serde_json::Value,
    }
    #[derive(Deserialize)]
    struct Version1 {
        #[serde(default)]
        skipped: bool,
        #[serde(default)]
        summary: Option<String>,
        #[serde(default)]
        findings: Option<serde_json::Value>,
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|_| ResultError::Invalid)?;
    let versioned = Versioned::deserialize(&value).map_err(|_| ResultError::Invalid)?;
    match versioned.version {
        serde_json::Value::Number(n) if n.as_u64() == Some(VERSION) => {}
        serde_json::Value::Number(_) => return Err(ResultError::UnsupportedVersion),
        _ => return Err(ResultError::Invalid),
    }
    let result = Version1::deserialize(&value).map_err(|_| ResultError::Invalid)?;
    Ok(JobResult {
        skipped: result.skipped,
        summary: result.summary.as_deref().and_then(Summary::new),
        findings: result
            .findings
            .as_ref()
            .map(findings::parse)
            .transpose()
            .map_err(|_| ResultError::Invalid)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{DirBuilderExt, symlink};

    #[test]
    fn reads_version_1_and_ignores_unknown_fields() {
        assert_eq!(
            parse(br#"{"version": 1, "skipped": true, "summary": " nothing new ", "extra": []}"#),
            Ok(JobResult {
                skipped: true,
                summary: Summary::new("nothing new"),
                findings: None,
            })
        );
        assert_eq!(parse(br#"{"version": 1}"#), Ok(JobResult::default()));
        assert_eq!(
            parse(br#"{"version": 1, "summary": null, "skipped": false}"#),
            Ok(JobResult::default())
        );
        assert_eq!(
            parse(br#"{"version": 1, "summary": ""}"#),
            Ok(JobResult::default())
        );
    }

    #[test]
    fn refuses_other_shapes_and_versions() {
        for invalid in [
            &b""[..],
            b"not json",
            b"[]",
            b"{}",
            br#"{"version": "1"}"#,
            br#"{"version": 1, "skipped": "yes"}"#,
            br#"{"version": 1, "summary": 3}"#,
            br#"{"version": 1} trailing"#,
            br#"{"version": 1, "findings": {}}"#,
            br#"{"version": 1, "findings": [{"key": "rhythm:result", "title": "t"}]}"#,
        ] {
            assert_eq!(
                parse(invalid),
                Err(ResultError::Invalid),
                "{}",
                String::from_utf8_lossy(invalid)
            );
        }
        for unsupported in [
            &br#"{"version": 2}"#[..],
            br#"{"version": 0}"#,
            br#"{"version": 1.5}"#,
        ] {
            assert_eq!(parse(unsupported), Err(ResultError::UnsupportedVersion));
        }
    }

    #[test]
    fn findings_count_whatever_the_outcome_and_null_means_none() {
        let result = parse(br#"{"version": 1, "findings": [{"key": "k", "title": "t"}]}"#).unwrap();
        let findings = result.findings.clone().unwrap();
        assert_eq!(findings.problems[0].key.as_str(), "k");
        assert_eq!(result.collected().findings, Said::Readable(Some(findings)));
        let silent = parse(br#"{"version": 1, "findings": null}"#).unwrap();
        assert_eq!(silent.collected().findings, Said::Readable(None));
    }

    fn private_dir() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::TempDir::new().unwrap();
        let dir = temp.path().join("run");
        std::fs::DirBuilder::new().mode(0o700).create(&dir).unwrap();
        (temp, dir)
    }

    #[test]
    fn reads_only_a_bounded_regular_file() {
        let (temp, dir) = private_dir();
        assert_eq!(read(&dir), Ok(None));
        assert_eq!(collect(&dir), Collected::nothing());

        let target = temp.path().join("elsewhere.json");
        std::fs::write(&target, r#"{"version": 1, "skipped": true}"#).unwrap();
        symlink(&target, path(&dir)).unwrap();
        assert_eq!(read(&dir), Err(ResultError::Unreadable));
        std::fs::remove_file(path(&dir)).unwrap();

        std::fs::create_dir(path(&dir)).unwrap();
        assert_eq!(read(&dir), Err(ResultError::Unreadable));
        std::fs::remove_dir(path(&dir)).unwrap();

        let fifo = std::ffi::CString::new(path(&dir).to_str().unwrap()).unwrap();
        assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);
        assert_eq!(read(&dir), Err(ResultError::Unreadable));
        std::fs::remove_file(path(&dir)).unwrap();

        let padding = " ".repeat(MAX_BYTES as usize - 14);
        std::fs::write(path(&dir), format!(r#"{{"version": 1}}{padding}"#)).unwrap();
        assert_eq!(read(&dir), Ok(Some(JobResult::default())));
        std::fs::write(path(&dir), format!(r#"{{"version": 1}}{padding} "#)).unwrap();
        assert_eq!(read(&dir), Err(ResultError::TooLarge));
        assert_eq!(
            collect(&dir),
            Collected {
                report: RunReport {
                    result_error: Some(ResultError::TooLarge),
                    ..RunReport::default()
                },
                findings: Said::Unreadable(ResultError::TooLarge),
            }
        );
    }
}
