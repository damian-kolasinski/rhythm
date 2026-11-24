//! Launch requests and the durable record a routine and its session share.
mod launchers;
mod store;

use crate::{
    operation_error::{Error, Result},
    runs::Timestamp,
    terminal_text,
};
use serde::{Deserialize, Serialize};
use std::{
    path::{Path, PathBuf},
    str::FromStr,
};

pub use launchers::LocalLaunchers;
pub use store::Store;

pub const RUN_ID: &str = "RHYTHM_RUN_ID";
pub const DATA_DIR: &str = "RHYTHM_RUN_DATA_DIR";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "backend", rename_all = "snake_case")]
pub enum Profile {
    Claude {
        executable: Option<PathBuf>,
        model: Option<String>,
    },
    T3 {
        executable: Option<PathBuf>,
        project: Option<String>,
        data_dir: Option<PathBuf>,
        model: Option<Model>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Model {
    pub provider: String,
    pub name: String,
}

impl Profile {
    pub const DEFAULT: Self = Self::Claude {
        executable: None,
        model: None,
    };

    pub fn kind(&self) -> Backend {
        match self {
            Self::Claude { .. } => Backend::Claude,
            Self::T3 { .. } => Backend::T3,
        }
    }

    pub fn validate(&self) -> Result<()> {
        let valid_path = |path: &Path| path.is_absolute() && path.to_str().is_some();
        let valid_option = |value: &str| text(value, 250).is_ok() && !value.starts_with('-');
        let valid = match self {
            Self::Claude { executable, model } => {
                executable.as_deref().is_none_or(valid_path)
                    && model.as_deref().is_none_or(valid_option)
            }
            Self::T3 {
                executable,
                project,
                data_dir,
                model,
            } => {
                executable.as_deref().is_none_or(valid_path)
                    && data_dir.as_deref().is_none_or(valid_path)
                    && project.as_deref().is_none_or(valid_option)
                    && model.as_ref().is_none_or(|model| {
                        valid_option(&model.provider) && valid_option(&model.name)
                    })
            }
        };
        if valid {
            Ok(())
        } else {
            Err(Error::InvalidRequest)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Backend {
    Claude,
    T3,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SessionId(String);

impl SessionId {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl FromStr for SessionId {
    type Err = Error;

    fn from_str(value: &str) -> Result<Self> {
        if value.len() == 32 && value.bytes().all(|b| b.is_ascii_hexdigit()) {
            Ok(Self(value.to_ascii_lowercase()))
        } else {
            Err(Error::InvalidRequest)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Launch {
    pub provider_id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub link: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub branch: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Outcome {
    pub kind: OutcomeKind,
    pub summary: String,
    pub pull_requests: Vec<String>,
    pub reported_at: Timestamp,
}

impl Outcome {
    pub fn new(kind: OutcomeKind, summary: &str, pull_requests: Vec<String>) -> Result<Self> {
        let summary = text(summary, 2000)?;
        if pull_requests.len() > 20 || pull_requests.iter().any(|url| !web_link(url)) {
            return Err(Error::InvalidRequest);
        }
        Ok(Self {
            kind,
            summary,
            pull_requests,
            reported_at: Timestamp::now(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum OutcomeKind {
    Done,
    Waiting,
    Failed,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Session {
    pub id: SessionId,
    pub title: String,
    pub backend: Backend,
    pub created_at: Timestamp,
    pub run_id: Option<i64>,
    pub launch: Option<Launch>,
    pub outcome: Option<Outcome>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionRun {
    pub id: i64,
    pub count: u32,
    pub started_at: Timestamp,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Activity {
    Running,
    Waiting,
    Idle,
    Ended,
    Missing,
    Unknown,
}

#[derive(Debug, Clone, Serialize)]
pub struct Details {
    #[serde(flatten)]
    pub session: Session,
    pub activity: Activity,
}

#[derive(Debug, Clone)]
pub struct Request {
    pub key: String,
    pub title: String,
    pub repository: PathBuf,
    pub base: String,
    pub add_dirs: Vec<PathBuf>,
    pub prompt: String,
}

impl Request {
    pub fn validate(&self) -> Result<()> {
        key(&self.key)?;
        text(&self.title, 200)?;
        if self.title.starts_with('-')
            || self.base.starts_with('-')
            || text(&self.base, 250).is_err()
            || !self.repository.is_absolute()
            || self.repository.to_str().is_none()
            || self.add_dirs.len() > 10
            || self.add_dirs.iter().any(|path| !path.is_absolute())
            || self.prompt.len() > 48 * 1024
            || self.prompt.trim().is_empty()
            || self.prompt.contains('\0')
        {
            return Err(Error::InvalidRequest);
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LaunchFailure {
    /// The launcher process could not start, or workspace setup failed.
    BeforeLaunch(Error),
    /// Creation may have happened. Keep the reservation to prevent duplicates.
    Unconfirmed,
}

pub trait Launcher {
    fn resolve(&self, profile: Profile) -> Profile;
    fn start(
        &self,
        profile: &Profile,
        request: &Request,
        id: &SessionId,
        prompt: &str,
    ) -> std::result::Result<Launch, LaunchFailure>;
    fn observe(&self, profile: &Profile, id: &SessionId, launch: &mut Launch) -> Activity;
}

pub struct Spawn<'a> {
    pub request: Request,
    pub profile: &'a str,
    pub run: Option<(i64, &'a str)>,
    pub reporting_command: &'a str,
}

pub fn spawn(store: &mut Store, launcher: &impl Launcher, input: Spawn<'_>) -> Result<Session> {
    let Spawn {
        request,
        profile,
        run,
        reporting_command,
    } = input;
    request.validate()?;
    key(profile)?;
    let scope = match run {
        Some((_, label)) => format!("job:{label}"),
        None => format!("repo:{}", request.repository.display()),
    };
    let (session, profile) = match store.reserve(
        &scope,
        &request,
        profile,
        run.map(|(id, _)| id),
        |profile| launcher.resolve(profile),
    )? {
        store::Reservation::Existing(session) if session.launch.is_some() => {
            return Ok(session);
        }
        store::Reservation::Existing(_) => return Err(Error::SessionLaunchUnconfirmed),
        store::Reservation::New(session, profile) => (session, profile),
    };
    let pull_request = "--pr https://example.com/owner/repo/pull/123";
    let id = session.id.as_str();
    let prompt = format!(
        "{}\n\nRecord the outcome in Rhythm with `{reporting_command} {id} --state done --summary 'What you accomplished and verified'`. Include each pull request with `{pull_request}` (using its actual URL). Report `--state waiting` if you need a decision, or `--state failed` if you cannot finish. Update this same record when the outcome changes.",
        request.prompt
    );
    match launcher.start(&profile, &request, &session.id, &prompt) {
        Ok(launch) => store.confirm(&session.id, &launch),
        Err(LaunchFailure::BeforeLaunch(error)) => {
            store.release(&session.id)?;
            Err(error)
        }
        Err(LaunchFailure::Unconfirmed) => Err(Error::SessionLaunchUnconfirmed),
    }
}

pub fn details(store: &Store, launcher: &impl Launcher, id: &SessionId) -> Result<Details> {
    let mut stored = store.get(id)?;
    let activity = stored
        .session
        .launch
        .as_mut()
        .map_or(Activity::Unknown, |launch| {
            launcher.observe(&stored.profile, id, launch)
        });
    Ok(Details {
        session: stored.session,
        activity,
    })
}

fn key(value: &str) -> Result<()> {
    if !value.is_empty()
        && value.len() <= 200
        && value
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.:/@".contains(&b))
    {
        Ok(())
    } else {
        Err(Error::InvalidRequest)
    }
}

fn text(value: &str, limit: usize) -> Result<String> {
    let value = value.trim();
    if value.is_empty() || value.len() > limit || terminal_text::clean(value) != value {
        return Err(Error::InvalidRequest);
    }
    Ok(value.to_owned())
}

pub fn web_link(value: &str) -> bool {
    let Some((scheme, rest)) = value.split_once("://") else {
        return false;
    };
    let authority = rest.split(['/', '?', '#']).next().unwrap_or_default();
    matches!(scheme, "http" | "https")
        && !authority.is_empty()
        && !authority.contains('@')
        && value.len() <= 1000
        && value.bytes().all(|b| b.is_ascii_graphic())
        && !value.contains(['\\', '"', '<', '>', '`'])
}
