//! SQLite record of runs, shared by every recorder and reader on the machine.
use crate::{
    jobs::Label,
    operation_error::{Error, Result},
    private_storage::{self, Root},
    retention::{self, Stored},
    runs::{Outcome, RecentRuns, ResultError, Run, RunReport, RunState, Summary, Timestamp},
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

const FILE: &str = "ledger.sqlite3";
/// Version 2 adds what runs report about themselves. Migrations only go
/// forward: an older Rhythm refuses a newer ledger as `unsupported_schema`.
const SCHEMA: i64 = 2;
const CREATE_1: &str = "CREATE TABLE IF NOT EXISTS runs (
                          id INTEGER PRIMARY KEY AUTOINCREMENT,
                          label TEXT NOT NULL,
                          started_at INTEGER NOT NULL,
                          recorder_pid INTEGER NOT NULL,
                          ended_at INTEGER,
                          outcome TEXT,
                          CHECK ((ended_at IS NULL) = (outcome IS NULL))
                        );
                        CREATE INDEX IF NOT EXISTS runs_by_label ON runs (label, id);";
const MIGRATE_2: &str = "ALTER TABLE runs ADD COLUMN skipped INTEGER NOT NULL DEFAULT 0;
                         ALTER TABLE runs ADD COLUMN summary TEXT;
                         ALTER TABLE runs ADD COLUMN result_error TEXT;";
const COLUMNS: &str =
    "id, label, started_at, recorder_pid, ended_at, outcome, skipped, summary, result_error";
const LOCK: &str = "recorder.lock";
const OPEN_LOCK: &str = "ledger.lock";
/// How long readers wait for a busy ledger.
pub const READER_WAIT: Duration = Duration::from_secs(10);
/// A recorder waits briefly: a busy ledger must not delay the job much.
pub const RECORDER_WAIT: Duration = Duration::from_secs(2);

pub struct Ledger {
    connection: Connection,
    root: Root,
}
impl Ledger {
    pub fn open(root: Root, wait: Duration) -> Result<Self> {
        let path = root.path().join(FILE);
        // Switching a new database to WAL and creating its schema race between
        // first openers without SQLite's busy handling, so openers take turns.
        let guard = private_storage::private_file(&root.path().join(OPEN_LOCK))?;
        if unsafe { libc::flock(guard.as_raw_fd(), libc::LOCK_EX) } != 0 {
            return Err(Error::StorageUnavailable);
        }
        private_storage::private_file(&path)?;
        let ledger = Self::connect(root, path, wait)?;
        ledger.migrate()?;
        Ok(ledger)
    }
    /// For readers: a machine that never recorded a run has no ledger.
    pub fn open_existing(root: Root) -> Result<Option<Self>> {
        let path = root.path().join(FILE);
        if !path.try_exists().map_err(|_| Error::StorageUnavailable)? {
            return Ok(None);
        }
        Self::open(root, READER_WAIT).map(Some)
    }
    fn connect(root: Root, path: PathBuf, wait: Duration) -> Result<Self> {
        let connection = Connection::open(path).map_err(|_| Error::StorageUnavailable)?;
        connection
            .busy_timeout(wait)
            .and_then(|_| connection.pragma_update(None, "journal_mode", "WAL"))
            .and_then(|_| connection.pragma_update(None, "synchronous", "NORMAL"))
            .map_err(|_| Error::StorageUnavailable)?;
        Ok(Self { connection, root })
    }
    fn migrate(&self) -> Result<()> {
        let version: i64 = self
            .connection
            .query_row("PRAGMA user_version", [], |r| r.get(0))
            .map_err(|_| Error::StorageUnavailable)?;
        let steps = match version {
            SCHEMA => return Ok(()),
            0 => [CREATE_1, MIGRATE_2].concat(),
            1 => MIGRATE_2.to_owned(),
            _ => return Err(Error::UnsupportedSchema),
        };
        self.connection
            .execute_batch(&format!(
                "BEGIN IMMEDIATE; {steps} PRAGMA user_version = {SCHEMA}; COMMIT;"
            ))
            .map_err(|_| {
                let _ = self.connection.execute_batch("ROLLBACK");
                Error::StorageUnavailable
            })
    }

    pub fn start(&self, label: &Label, recorder_pid: u32, at: Timestamp) -> Result<i64> {
        self.connection
            .execute(
                "INSERT INTO runs (label, started_at, recorder_pid) VALUES (?1, ?2, ?3)",
                params![label.as_str(), at.0, recorder_pid],
            )
            .map_err(|_| Error::StorageUnavailable)?;
        let id = self.connection.last_insert_rowid();
        // Whether this run will be skipped is not known yet, so it does not
        // count yet; pruning now keeps runs that are always lost in check.
        let _ = self.prune(id, at, Some(id));
        Ok(id)
    }
    /// Records how the run ended together with what it reported about itself.
    pub fn finish(
        &self,
        id: i64,
        at: Timestamp,
        outcome: Outcome,
        report: &RunReport,
    ) -> Result<()> {
        let skipped = RunState::Finished {
            ended_at: at,
            outcome,
            report: report.clone(),
        }
        .skipped();
        self.connection
            .execute(
                "UPDATE runs SET ended_at = ?1, outcome = ?2, skipped = ?3, summary = ?4,
                   result_error = ?5
                 WHERE id = ?6 AND ended_at IS NULL",
                params![
                    at.0,
                    serde_json::to_string(&outcome).unwrap(),
                    skipped,
                    report.summary.as_ref().map(Summary::as_str),
                    report.result_error.map(ResultError::code),
                    id
                ],
            )
            .map_err(|_| Error::StorageUnavailable)?;
        // Only now is it known whether the run was skipped. The outcome is
        // recorded either way; the next run retries a failed prune.
        let _ = self.prune(id, at, None);
        Ok(())
    }
    /// Drops the runs of `id`'s job beyond what [`retention`] keeps, with
    /// their output, leaving `pending` aside.
    fn prune(&self, id: i64, at: Timestamp, pending: Option<i64>) -> Result<()> {
        let transaction = || -> rusqlite::Result<Vec<i64>> {
            let stored: Vec<Stored> = self
                .connection
                .prepare(
                    "SELECT id, started_at, ended_at IS NOT NULL, skipped FROM runs
                     WHERE label = (SELECT label FROM runs WHERE id = ?1) AND id != ?2
                     ORDER BY id DESC",
                )?
                .query_map(params![id, pending.unwrap_or(0)], |r| {
                    Ok(Stored {
                        id: r.get(0)?,
                        started_at: Timestamp(r.get(1)?),
                        finished: r.get(2)?,
                        skipped: r.get(3)?,
                    })
                })?
                .collect::<rusqlite::Result<_>>()?;
            let expired = retention::expired(&stored, at);
            for old in &expired {
                self.connection
                    .execute("DELETE FROM runs WHERE id = ?1", [old])?;
            }
            Ok(expired)
        };
        self.connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| Error::StorageUnavailable)?;
        let expired = match transaction() {
            Ok(expired) => expired,
            Err(_) => {
                let _ = self.connection.execute_batch("ROLLBACK");
                return Err(Error::StorageUnavailable);
            }
        };
        self.connection
            .execute_batch("COMMIT")
            .map_err(|_| Error::StorageUnavailable)?;
        for old in expired {
            let _ = std::fs::remove_dir_all(self.output_location(old));
        }
        Ok(())
    }
    /// The private directory holding a run's output, created on first use.
    pub fn output_dir(&self, id: i64) -> Result<PathBuf> {
        private_storage::private_dir(&self.root.path().join("runs"))?;
        let dir = self.output_location(id);
        private_storage::private_dir(&dir)?;
        Ok(dir)
    }
    /// Where a run's output is, whether or not it was written.
    pub fn output_location(&self, id: i64) -> PathBuf {
        self.root.path().join("runs").join(id.to_string())
    }

    /// The job's last run before `id` that says whether the job worked: lost,
    /// stopped, and skipped runs do not.
    pub fn previous_finished(&self, label: &str, id: i64) -> Result<Option<Run>> {
        self.newest_with_verdict(label, &format!("AND id < {id} AND ended_at IS NOT NULL"))
    }
    /// The newest run matching `filter` that says whether the job works; see
    /// [`RunState::verdict`].
    fn newest_with_verdict(&self, label: &str, filter: &str) -> Result<Option<Run>> {
        self.connection
            .prepare(&format!(
                "SELECT {COLUMNS} FROM runs WHERE label = ?1 AND skipped = 0 {filter}
                 ORDER BY id DESC"
            ))
            .and_then(|mut s| {
                s.query_map([label], |r| run(r, &self.root))?
                    .find(|r| r.as_ref().map_or(true, |run| run.state.verdict().is_some()))
                    .transpose()
            })
            .map_err(|_| Error::StorageUnavailable)
    }
    /// Runs of the job after `id` that failed or were lost, oldest first.
    pub fn failures_after(&self, label: &str, id: i64) -> Result<Vec<Run>> {
        let mut runs: Vec<Run> = self
            .connection
            .prepare(&format!(
                "SELECT {COLUMNS} FROM runs WHERE label = ?1 AND id > ?2 ORDER BY id"
            ))
            .and_then(|mut s| {
                s.query_map(params![label, id], |r| run(r, &self.root))?
                    .collect()
            })
            .map_err(|_| Error::StorageUnavailable)?;
        runs.retain(|r| r.state.failed());
        Ok(runs)
    }
    /// The newest run, skipped or not.
    pub fn latest(&self, label: &str) -> Result<Option<Run>> {
        self.newest(label, "")
    }
    /// The job's newest real run, a skip that followed it, and the newest
    /// verdict on whether the job works.
    pub fn recent(&self, label: &str) -> Result<RecentRuns> {
        // One snapshot, so a run finishing meanwhile cannot mix two states.
        let snapshot = self
            .connection
            .unchecked_transaction()
            .map_err(|_| Error::StorageUnavailable)?;
        let last_run = self.newest(label, "AND skipped = 0")?;
        let after = last_run.as_ref().map_or(0, |r| r.id);
        let last_skip = self.newest(label, &format!("AND skipped = 1 AND id > {after}"))?;
        let verdict = self
            .newest_with_verdict(label, "")?
            .and_then(|run| run.state.verdict());
        snapshot.commit().map_err(|_| Error::StorageUnavailable)?;
        Ok(RecentRuns {
            last_run,
            last_skip,
            verdict,
        })
    }
    fn newest(&self, label: &str, filter: &str) -> Result<Option<Run>> {
        self.connection
            .query_row(
                &format!(
                    "SELECT {COLUMNS} FROM runs WHERE label = ?1 {filter} ORDER BY id DESC LIMIT 1"
                ),
                [label],
                |r| run(r, &self.root),
            )
            .optional()
            .map_err(|_| Error::StorageUnavailable)
    }
    /// The newest `limit` runs that were not skipped, with the skipped runs
    /// among and after them; newest first.
    pub fn history(&self, label: &str, limit: u32) -> Result<Vec<Run>> {
        let oldest: i64 = self
            .connection
            .query_row(
                "SELECT id FROM runs WHERE label = ?1 AND skipped = 0
                 ORDER BY id DESC LIMIT 1 OFFSET ?2",
                params![label, limit.saturating_sub(1)],
                |r| r.get(0),
            )
            .optional()
            .map_err(|_| Error::StorageUnavailable)?
            .unwrap_or(0);
        self.connection
            .prepare(&format!(
                "SELECT {COLUMNS} FROM runs WHERE label = ?1 AND id >= ?2 ORDER BY id DESC"
            ))
            .and_then(|mut s| {
                s.query_map(params![label, oldest], |r| run(r, &self.root))?
                    .collect()
            })
            .map_err(|_| Error::StorageUnavailable)
    }
    pub fn run(&self, id: i64) -> Result<Option<Run>> {
        self.connection
            .query_row(
                &format!("SELECT {COLUMNS} FROM runs WHERE id = ?1"),
                [id],
                |r| run(r, &self.root),
            )
            .optional()
            .map_err(|_| Error::StorageUnavailable)
    }
}

fn run(row: &Row, root: &Root) -> rusqlite::Result<Run> {
    let ended_at: Option<i64> = row.get(4)?;
    let outcome: Option<String> = row.get(5)?;
    let state = match (ended_at, outcome) {
        (Some(ended_at), Some(outcome)) => RunState::Finished {
            ended_at: Timestamp(ended_at),
            outcome: serde_json::from_str(&outcome).map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
            })?,
            report: RunReport {
                skipped: row.get(6)?,
                summary: row
                    .get::<_, Option<String>>(7)?
                    .and_then(|text| Summary::new(&text)),
                result_error: row
                    .get::<_, Option<String>>(8)?
                    .and_then(|code| ResultError::from_code(&code)),
            },
        },
        _ => {
            let id: i64 = row.get(0)?;
            let dir = root.path().join("runs").join(id.to_string());
            let pid: i64 = row.get(3)?;
            let alive = RunLock::held(&dir).unwrap_or_else(|| recorder_alive(pid));
            if alive {
                RunState::Running
            } else {
                RunState::Lost
            }
        }
    };
    Ok(Run {
        id: row.get(0)?,
        label: row.get(1)?,
        started_at: Timestamp(row.get(2)?),
        state,
    })
}

/// Held by a recorder for as long as it runs; the kernel releases it however
/// the recorder ends, which makes it a reliable sign of a lost run.
pub struct RunLock(#[allow(dead_code)] File);
impl RunLock {
    pub fn hold(dir: &Path) -> Result<Self> {
        let file = private_storage::private_file(&dir.join(LOCK))?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
            return Err(Error::StorageUnavailable);
        }
        Ok(Self(file))
    }
    /// `None` when the run never created its lock.
    fn held(dir: &Path) -> Option<bool> {
        let file = File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join(LOCK))
            .ok()?;
        if unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_SH | libc::LOCK_NB) } == 0 {
            unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_UN) };
            Some(false)
        } else {
            Some(std::io::Error::last_os_error().kind() == std::io::ErrorKind::WouldBlock)
        }
    }
}

/// Fallback for runs without a lock: any process of this user with that PID.
fn recorder_alive(pid: i64) -> bool {
    let Ok(pid) = libc::pid_t::try_from(pid) else {
        return false;
    };
    pid > 0 && unsafe { libc::kill(pid, 0) } == 0
}
