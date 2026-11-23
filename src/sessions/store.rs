//! Independent session storage. Detaching a session from run output retention
//! lets it report an outcome after the originating command has exited.
use super::{Launch, Outcome, Profile, Request, Session, SessionId, SessionRun, key};
use crate::{
    operation_error::{Error, Result},
    private_storage::{self, Root},
    runs::Timestamp,
};
use rusqlite::{Connection, OpenFlags, OptionalExtension, Row, params};
use serde::{Serialize, de::DeserializeOwned};
use std::time::Duration;

const FILE: &str = "sessions.sqlite3";
const SCHEMA: &str = "
    CREATE TABLE IF NOT EXISTS profiles (name TEXT PRIMARY KEY, config TEXT NOT NULL);
    CREATE TABLE IF NOT EXISTS sessions (
        id TEXT PRIMARY KEY,
        scope TEXT NOT NULL,
        task_key TEXT NOT NULL,
        title TEXT NOT NULL,
        profile TEXT NOT NULL,
        created_at INTEGER NOT NULL,
        run_id INTEGER,
        launch TEXT,
        outcome TEXT,
        UNIQUE(scope, task_key)
    );
    CREATE INDEX IF NOT EXISTS sessions_by_run ON sessions(run_id);
";
const COLUMNS: &str = "id, title, profile, created_at, run_id, launch, outcome";

pub struct Store(Connection);

pub(super) struct Stored {
    pub session: Session,
    pub profile: Profile,
}

pub(super) enum Reservation {
    New(Session, Profile),
    Existing(Session),
}

impl Store {
    pub fn open(root: &Root) -> Result<Self> {
        let path = root.path().join(FILE);
        private_storage::private_file(&path)?;
        let db = Connection::open(path).map_err(storage)?;
        db.busy_timeout(Duration::from_secs(3)).map_err(storage)?;
        db.execute_batch(SCHEMA).map_err(storage)?;
        Ok(Self(db))
    }

    pub fn existing(root: &Root) -> Result<Option<Self>> {
        let path = root.path().join(FILE);
        match std::fs::symlink_metadata(&path) {
            Ok(_) => {
                private_storage::read_private_file(&path)?;
                let db = Connection::open_with_flags(path, OpenFlags::SQLITE_OPEN_READ_ONLY)
                    .map_err(storage)?;
                db.busy_timeout(Duration::from_secs(3)).map_err(storage)?;
                Ok(Some(Self(db)))
            }
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(_) => Err(Error::StorageUnavailable),
        }
    }

    pub(crate) fn count_for_run(&self, id: i64) -> Result<u32> {
        self.0
            .query_row(
                "SELECT COUNT(*) FROM sessions WHERE run_id=? AND launch IS NOT NULL",
                [id],
                |row| row.get(0),
            )
            .map_err(storage)
    }

    pub fn latest_for_job(&self, job: &str) -> Result<Option<SessionRun>> {
        self.0
            .query_row(
                "SELECT run_id, COUNT(*), MIN(created_at) FROM sessions
             WHERE scope=? AND run_id IS NOT NULL AND launch IS NOT NULL
             GROUP BY run_id ORDER BY run_id DESC LIMIT 1",
                [format!("job:{job}")],
                |row| {
                    Ok(SessionRun {
                        id: row.get(0)?,
                        count: row.get(1)?,
                        started_at: Timestamp(row.get(2)?),
                    })
                },
            )
            .optional()
            .map_err(storage)
    }

    pub fn set_profile(&self, name: &str, profile: &Profile) -> Result<()> {
        key(name)?;
        profile.validate()?;
        self.0.execute(
            "INSERT INTO profiles VALUES (?1, ?2) ON CONFLICT(name) DO UPDATE SET config=excluded.config",
            params![name, encode(profile)?],
        ).map_err(storage)?;
        Ok(())
    }

    pub fn profile(&self, name: &str) -> Result<Profile> {
        key(name)?;
        profile(&self.0, name)
    }

    pub fn for_run(&self, id: i64) -> Result<Vec<Session>> {
        self.list_run(id, false)
    }

    pub fn list_run(&self, id: i64, include_unconfirmed: bool) -> Result<Vec<Session>> {
        let mut query = self.0.prepare(&format!(
            "SELECT {COLUMNS} FROM sessions WHERE run_id=?1 AND (?2 OR launch IS NOT NULL) ORDER BY created_at, rowid"
        )).map_err(storage)?;
        query
            .query_map(params![id, include_unconfirmed], |row| {
                stored(row).map(|stored| stored.session)
            })
            .map_err(storage)?
            .collect::<rusqlite::Result<_>>()
            .map_err(storage)
    }

    pub fn recorded(&self, id: &SessionId) -> Result<Session> {
        Ok(self.get(id)?.session)
    }

    pub(super) fn get(&self, id: &SessionId) -> Result<Stored> {
        self.0
            .query_row(
                &format!("SELECT {COLUMNS} FROM sessions WHERE id=?"),
                [id.as_str()],
                stored,
            )
            .optional()
            .map_err(storage)?
            .ok_or(Error::SessionNotFound)
    }

    pub(super) fn reserve(
        &mut self,
        scope: &str,
        request: &Request,
        name: &str,
        run: Option<i64>,
        resolve: impl FnOnce(Profile) -> Profile,
    ) -> Result<Reservation> {
        let tx = self
            .0
            .transaction_with_behavior(rusqlite::TransactionBehavior::Immediate)
            .map_err(storage)?;
        if let Some(row) = tx
            .query_row(
                &format!("SELECT {COLUMNS} FROM sessions WHERE scope=? AND task_key=?"),
                params![scope, request.key],
                stored,
            )
            .optional()
            .map_err(storage)?
        {
            return Ok(Reservation::Existing(row.session));
        }
        let profile = resolve(profile(&tx, name)?);
        let row = tx
            .query_row(
                &format!(
                    "INSERT INTO sessions (id,scope,task_key,title,profile,created_at,run_id)
                 VALUES (lower(hex(randomblob(16))),?1,?2,?3,?4,?5,?6) RETURNING {COLUMNS}"
                ),
                params![
                    scope,
                    request.key,
                    request.title,
                    encode(&profile)?,
                    Timestamp::now().0,
                    run
                ],
                stored,
            )
            .map_err(storage)?;
        tx.commit().map_err(storage)?;
        Ok(Reservation::New(row.session, profile))
    }

    pub(super) fn confirm(&self, id: &SessionId, launch: &Launch) -> Result<Session> {
        self.0
            .execute(
                "UPDATE sessions SET launch=? WHERE id=?",
                params![encode(launch)?, id.as_str()],
            )
            .map_err(storage)?;
        self.recorded(id)
    }

    pub(super) fn release(&self, id: &SessionId) -> Result<()> {
        self.0
            .execute(
                "DELETE FROM sessions WHERE id=? AND launch IS NULL AND outcome IS NULL",
                [id.as_str()],
            )
            .map_err(storage)?;
        Ok(())
    }

    pub fn forget_unconfirmed(&self, id: &SessionId, before: Timestamp) -> Result<()> {
        let deleted = self.0.execute(
            "DELETE FROM sessions WHERE id=? AND launch IS NULL AND outcome IS NULL AND created_at <= ?",
            params![id.as_str(), before.0],
        ).map_err(storage)?;
        if deleted == 0 {
            return Err(Error::SessionResetRefused);
        }
        Ok(())
    }

    pub fn report(&self, id: &SessionId, outcome: &Outcome) -> Result<Session> {
        let changed = self
            .0
            .execute(
                "UPDATE sessions SET outcome=? WHERE id=?",
                params![encode(outcome)?, id.as_str()],
            )
            .map_err(storage)?;
        if changed == 0 {
            return Err(Error::SessionNotFound);
        }
        self.recorded(id)
    }
}

fn profile(db: &Connection, name: &str) -> Result<Profile> {
    let row = db
        .query_row("SELECT config FROM profiles WHERE name=?", [name], |row| {
            decode(row, 0)
        })
        .optional()
        .map_err(storage)?;
    row.or_else(|| (name == "default").then_some(Profile::DEFAULT))
        .ok_or(Error::SessionProfileNotFound)
}

fn stored(row: &Row) -> rusqlite::Result<Stored> {
    let profile: Profile = decode(row, 2)?;
    let launch = optional(row, 5)?;
    let outcome = optional(row, 6)?;
    Ok(Stored {
        session: Session {
            id: SessionId(row.get(0)?),
            title: row.get(1)?,
            backend: profile.kind(),
            created_at: Timestamp(row.get(3)?),
            run_id: row.get(4)?,
            launch,
            outcome,
        },
        profile,
    })
}

fn decode<T: DeserializeOwned>(row: &Row, index: usize) -> rusqlite::Result<T> {
    serde_json::from_str(&row.get::<_, String>(index)?).map_err(|error| {
        rusqlite::Error::FromSqlConversionFailure(
            index,
            rusqlite::types::Type::Text,
            Box::new(error),
        )
    })
}

fn optional<T: DeserializeOwned>(row: &Row, index: usize) -> rusqlite::Result<Option<T>> {
    let value: Option<String> = row.get(index)?;
    value
        .as_deref()
        .map(serde_json::from_str)
        .transpose()
        .map_err(|error| {
            rusqlite::Error::FromSqlConversionFailure(
                index,
                rusqlite::types::Type::Text,
                Box::new(error),
            )
        })
}

fn encode(value: &impl Serialize) -> Result<String> {
    serde_json::to_string(value).map_err(|_| Error::StorageUnavailable)
}

fn storage(_: rusqlite::Error) -> Error {
    Error::StorageUnavailable
}
