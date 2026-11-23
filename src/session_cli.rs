use clap::{Subcommand, ValueEnum};
use rhythm::{
    jobs::Label,
    ledger::Ledger,
    operation_error::{Error, Result},
    recorder, recorder_link,
    sessions::{
        self, LocalLaunchers, Model, Outcome, OutcomeKind, Profile, Request, SessionId, Spawn,
        Store,
    },
    state_directory,
};
use std::{
    io::{self, Read},
    path::{Path, PathBuf},
    process::ExitCode,
};

#[derive(Subcommand)]
pub enum SessionCommand {
    /// Choose a launcher for a named profile. The default profile starts Claude in the background.
    Configure {
        #[arg(default_value = "default")]
        profile: String,
        #[arg(long, value_enum)]
        backend: BackendArg,
        /// Absolute launcher path when the CLI is outside the routine's PATH.
        #[arg(long)]
        executable: Option<PathBuf>,
        #[arg(long)]
        model: Option<String>,
        /// T3 provider instance paired with a model override.
        #[arg(long)]
        provider: Option<String>,
        /// T3 project ID or absolute checkout path. Defaults to --repo at launch.
        #[arg(long)]
        project: Option<String>,
        /// T3 environment data directory.
        #[arg(long)]
        t3_data_dir: Option<PathBuf>,
    },
    /// Show the launcher configured for a profile.
    Profile {
        #[arg(default_value = "default")]
        profile: String,
    },
    /// Start a session in a fresh worktree, reading the first prompt from stdin.
    Spawn {
        #[arg(long, default_value = "default")]
        profile: String,
        #[arg(long)]
        key: String,
        #[arg(long)]
        title: String,
        #[arg(long)]
        repo: PathBuf,
        #[arg(long, default_value = "origin/main")]
        base: String,
        #[arg(long = "add-dir")]
        add_dirs: Vec<PathBuf>,
    },
    /// Show the recorded outcome and current provider activity.
    Show { id: SessionId },
    /// List confirmed sessions spawned by a recorded run.
    List {
        #[arg(long)]
        run: i64,
        #[arg(long)]
        include_unconfirmed: bool,
    },
    /// Release a key after checking that an unconfirmed launch created no session.
    ForgetUnconfirmed { id: SessionId },
    /// Record or update the outcome for a session.
    Report {
        id: SessionId,
        #[arg(long, value_enum)]
        state: StateArg,
        #[arg(long)]
        summary: String,
        #[arg(long = "pr")]
        pull_requests: Vec<String>,
    },
}

#[derive(Clone, Copy, ValueEnum)]
pub enum BackendArg {
    Claude,
    T3,
}
#[derive(Clone, Copy, ValueEnum)]
pub enum StateArg {
    Done,
    Waiting,
    Failed,
}
impl From<StateArg> for OutcomeKind {
    fn from(value: StateArg) -> Self {
        match value {
            StateArg::Done => Self::Done,
            StateArg::Waiting => Self::Waiting,
            StateArg::Failed => Self::Failed,
        }
    }
}

pub fn run(json: bool, data_dir: Option<PathBuf>, command: SessionCommand) -> ExitCode {
    let only_id = matches!(&command, SessionCommand::Spawn { .. });
    let inherited = std::env::var_os(sessions::DATA_DIR).map(PathBuf::from);
    let data_dir = data_dir.or_else(|| inherited.clone());
    let run = || -> Result<serde_json::Value> {
        match command {
            SessionCommand::Profile { profile } => {
                let store = state_directory::existing(data_dir)?
                    .as_ref()
                    .map(Store::existing)
                    .transpose()?
                    .flatten();
                let config = match store {
                    Some(store) => store.profile(&profile)?,
                    None if profile == "default" => Profile::DEFAULT,
                    None => return Err(Error::SessionProfileNotFound),
                };
                value(config)
            }
            SessionCommand::Configure {
                profile,
                backend,
                executable,
                model,
                provider,
                project,
                t3_data_dir,
            } => {
                let config = match backend {
                    BackendArg::Claude
                        if provider.is_none() && project.is_none() && t3_data_dir.is_none() =>
                    {
                        Profile::Claude { executable, model }
                    }
                    BackendArg::T3 if provider.is_some() == model.is_some() => Profile::T3 {
                        executable,
                        project,
                        data_dir: t3_data_dir,
                        model: provider
                            .zip(model)
                            .map(|(provider, name)| Model { provider, name }),
                    },
                    _ => return Err(Error::InvalidRequest),
                };
                Store::open(&state_directory::root(data_dir)?)?.set_profile(&profile, &config)?;
                value(config)
            }
            SessionCommand::Spawn {
                profile,
                key,
                title,
                repo,
                base,
                add_dirs,
            } => {
                let root = state_directory::root(data_dir)?;
                let mut store = Store::open(&root)?;
                let run = run_context(&root, inherited.as_deref())?;
                let executable = recorder_link::location(&root);
                let executable = if executable.exists() {
                    executable
                } else {
                    super::this_executable()?
                };
                let report = format!(
                    "{} --data-dir {} session report",
                    quote(&executable)?,
                    quote(root.path())?,
                );
                let mut prompt = String::new();
                io::stdin()
                    .take(48 * 1024 + 1)
                    .read_to_string(&mut prompt)
                    .map_err(|_| Error::InvalidRequest)?;
                let repo = if repo.is_absolute() {
                    repo.canonicalize()
                } else {
                    return Err(Error::InvalidRequest);
                }
                .map_err(|_| Error::SessionWorkspaceFailed)?;
                let request = Request {
                    key,
                    title,
                    repository: repo,
                    base,
                    add_dirs,
                    prompt,
                };
                let launcher = LocalLaunchers {
                    state_dir: root.path(),
                };
                value(sessions::spawn(
                    &mut store,
                    &launcher,
                    Spawn {
                        request,
                        profile: &profile,
                        run: run.as_ref().map(|(id, label)| (*id, label.as_str())),
                        reporting_command: &report,
                    },
                )?)
            }
            SessionCommand::List {
                run,
                include_unconfirmed,
            } => {
                let store = state_directory::existing(data_dir)?
                    .as_ref()
                    .map(Store::existing)
                    .transpose()?
                    .flatten();
                value(
                    store
                        .map(|store| store.list_run(run, include_unconfirmed))
                        .transpose()?
                        .unwrap_or_default(),
                )
            }
            SessionCommand::ForgetUnconfirmed { id } => {
                let root = state_directory::existing(data_dir)?.ok_or(Error::SessionNotFound)?;
                let before = rhythm::runs::Timestamp(rhythm::runs::Timestamp::now().0 - 300_000);
                Store::open(&root)?.forget_unconfirmed(&id, before)?;
                value(serde_json::json!({"forgotten": id}))
            }
            SessionCommand::Show { id } => {
                let root = state_directory::existing(data_dir)?.ok_or(Error::SessionNotFound)?;
                let store = Store::existing(&root)?.ok_or(Error::SessionNotFound)?;
                value(sessions::details(
                    &store,
                    &LocalLaunchers {
                        state_dir: root.path(),
                    },
                    &id,
                )?)
            }
            SessionCommand::Report {
                id,
                state,
                summary,
                pull_requests,
            } => {
                let root = state_directory::existing(data_dir)?.ok_or(Error::SessionNotFound)?;
                let store = Store::open(&root)?;
                value(store.report(&id, &Outcome::new(state.into(), &summary, pull_requests)?)?)
            }
        }
    };
    super::respond(json, run(), |value| {
        if only_id && let Some(id) = value.get("id").and_then(serde_json::Value::as_str) {
            format!("{id}\n")
        } else {
            super::to_json(value)
        }
    })
}

fn value(value: impl serde::Serialize) -> Result<serde_json::Value> {
    serde_json::to_value(value).map_err(|_| Error::StorageUnavailable)
}

fn run_context(
    root: &rhythm::private_storage::Root,
    inherited: Option<&Path>,
) -> Result<Option<(i64, Label)>> {
    let Some(id) = std::env::var(sessions::RUN_ID)
        .ok()
        .and_then(|id| id.parse().ok())
    else {
        return Ok(None);
    };
    if inherited != Some(root.path()) {
        return Err(Error::InvalidRequest);
    }
    let label = std::env::var(recorder::JOB)
        .map_err(|_| Error::InvalidRequest)?
        .parse::<Label>()?;
    let ledger = Ledger::open_existing(root.clone())?.ok_or(Error::NotFound)?;
    let run = ledger.run(id)?.ok_or(Error::NotFound)?;
    if run.label != label.as_str() || !matches!(run.state, rhythm::runs::RunState::Running) {
        return Err(Error::InvalidRequest);
    }
    Ok(Some((id, label)))
}

fn quote(path: &Path) -> Result<String> {
    let path = path.to_str().ok_or(Error::InvalidRequest)?;
    Ok(format!("'{}'", path.replace('\'', "'\\''")))
}
