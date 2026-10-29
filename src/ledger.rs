//! SQLite record of runs, shared by every recorder and reader on the machine.
use crate::{
    jobs::Label,
    operation_error::{Error, Result},
    private_storage::{self, Root},
    runs::{Outcome, Run, RunState, Timestamp},
};
use rusqlite::{Connection, OptionalExtension, Row, params};
use std::{
    fs::File,
    os::{fd::AsRawFd, unix::fs::OpenOptionsExt},
    path::{Path, PathBuf},
    time::Duration,
};

const FILE: &str = "ledger.sqlite3";
const SCHEMA: i64 = 1;
const LOCK: &str = "recorder.lock";
const OPEN_LOCK: &str = "ledger.lock";
/// Runs kept per job, with their output.
pub const KEEP: i64 = 100;
/// Unfinished runs younger than this are never pruned: they may still be running.
const UNFINISHED_GRACE_MS: i64 = 7 * 86_400_000;
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
        match version {
            SCHEMA => Ok(()),
            0 => self
                .connection
                .execute_batch(
                    "BEGIN IMMEDIATE;
                     CREATE TABLE IF NOT EXISTS runs (
                       id INTEGER PRIMARY KEY AUTOINCREMENT,
                       label TEXT NOT NULL,
                       started_at INTEGER NOT NULL,
                       recorder_pid INTEGER NOT NULL,
                       ended_at INTEGER,
                       outcome TEXT,
                       CHECK ((ended_at IS NULL) = (outcome IS NULL))
                     );
                     CREATE INDEX IF NOT EXISTS runs_by_label ON runs (label, id);
                     PRAGMA user_version = 1;
                     COMMIT;",
                )
                .map_err(|_| Error::StorageUnavailable),
            _ => Err(Error::UnsupportedSchema),
        }
    }

    /// Records a new run and drops the oldest finished ones beyond [`KEEP`] for its job.
    pub fn start(&self, label: &Label, recorder_pid: u32, at: Timestamp) -> Result<i64> {
        let transaction = || -> rusqlite::Result<(i64, Vec<i64>)> {
            self.connection.execute(
                "INSERT INTO runs (label, started_at, recorder_pid) VALUES (?1, ?2, ?3)",
                params![label.as_str(), at.0, recorder_pid],
            )?;
            let id = self.connection.last_insert_rowid();
            let expired: Vec<i64> = self
                .connection
                .prepare(
                    "SELECT id FROM runs WHERE label = ?1
                       AND (ended_at IS NOT NULL OR started_at < ?3)
                       AND id NOT IN (SELECT id FROM runs WHERE label = ?1 ORDER BY id DESC LIMIT ?2)",
                )?
                .query_map(
                    params![label.as_str(), KEEP, at.0 - UNFINISHED_GRACE_MS],
                    |r| r.get(0),
                )?
                .collect::<rusqlite::Result<_>>()?;
            for old in &expired {
                self.connection
                    .execute("DELETE FROM runs WHERE id = ?1", [old])?;
            }
            Ok((id, expired))
        };
        self.connection
            .execute_batch("BEGIN IMMEDIATE")
            .map_err(|_| Error::StorageUnavailable)?;
        let (id, expired) = match transaction() {
            Ok(result) => result,
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
        Ok(id)
    }
    pub fn finish(&self, id: i64, at: Timestamp, outcome: Outcome) -> Result<()> {
        let outcome = serde_json::to_string(&outcome).unwrap();
        self.connection
            .execute(
                "UPDATE runs SET ended_at = ?1, outcome = ?2 WHERE id = ?3 AND ended_at IS NULL",
                params![at.0, outcome, id],
            )
            .map_err(|_| Error::StorageUnavailable)
            .map(|_| ())
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

    pub fn latest(&self, label: &str) -> Result<Option<Run>> {
        Ok(self.history(label, 1)?.pop())
    }
    /// Newest first.
    pub fn history(&self, label: &str, limit: u32) -> Result<Vec<Run>> {
        self.connection
            .prepare(
                "SELECT id, label, started_at, recorder_pid, ended_at, outcome FROM runs
                 WHERE label = ?1 ORDER BY id DESC LIMIT ?2",
            )
            .and_then(|mut s| {
                s.query_map(params![label, limit], |r| run(r, &self.root))?
                    .collect()
            })
            .map_err(|_| Error::StorageUnavailable)
    }
    pub fn run(&self, id: i64) -> Result<Option<Run>> {
        self.connection
            .query_row(
                "SELECT id, label, started_at, recorder_pid, ended_at, outcome FROM runs
                 WHERE id = ?1",
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
