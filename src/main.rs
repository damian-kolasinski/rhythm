use clap::{Parser, Subcommand};
use rhythm::{
    attention::{self, AttentionHost, AttentionOutcome, AttentionResponse, Consumer},
    attention_render,
    attention_store::AttentionStore,
    authoring::{self, Added, Adoption, Caller, NewJob, OWNED_PREFIX, Recorder},
    history_render,
    jobs::{Label, RunStore},
    kick::{self, KickResponse},
    kick_render,
    launchd::Launchd,
    ledger::{self, Ledger},
    operation_error::{self, Error},
    recorder::{self, Recording},
    recorder_link,
    remote::{Host, Ssh},
    run_history,
    runs::Timestamp,
    schedule_input::{self, ScheduleInput},
    state_directory,
    status::{self, HostOutcome},
    status_render,
    targets::{self, Target},
};
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
    time::Duration,
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Machine-readable output; diagnostics are structured and source values are omitted.
    #[arg(long, global = true)]
    json: bool,
    /// Private directory for recorded runs (defaults to the user application state directory).
    #[arg(long, global = true, value_name = "ABSOLUTE_PATH")]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Show every user launchd job, those needing attention first.
    Status {
        /// Machine to inspect: `local` or an ssh destination. Repeat for several.
        #[arg(long = "host", value_name = "HOST")]
        hosts: Vec<String>,
    },
    /// Run COMMAND and record the run under LABEL; used in launchd definitions.
    Run {
        label: String,
        /// Also copy output to this process's stdout and stderr.
        #[arg(long)]
        forward: bool,
        #[arg(last = true, required = true, value_name = "COMMAND")]
        command: Vec<OsString>,
    },
    /// List jobs needing attention; with --since-last-seen, only what is new.
    Attention {
        /// Only report what changed since this consumer last looked.
        #[arg(long)]
        since_last_seen: bool,
        /// Who is looking, so each agent tool keeps its own place.
        #[arg(long = "as", default_value = "default", value_name = "NAME")]
        consumer: String,
        /// Machine to check: `local` or an ssh destination. Repeat for several.
        #[arg(long = "host", value_name = "HOST")]
        hosts: Vec<String>,
    },
    /// Create a launchd job that records its runs, labelled `dev.rhythm.NAME`.
    Add {
        name: String,
        /// Run every interval, as in 90s, 5m, 2h, or 1d.
        #[arg(long, value_name = "INTERVAL")]
        every: Option<String>,
        /// Run daily at a local 24-hour time. Repeatable.
        #[arg(long, value_name = "HH:MM")]
        daily: Vec<String>,
        /// Run weekly, as in mon@09:00. Repeatable.
        #[arg(long, value_name = "DAY@HH:MM")]
        weekly: Vec<String>,
        /// Also run when the job loads, such as at login.
        #[arg(long)]
        at_load: bool,
        /// Keep the command running, restarting it whenever it exits.
        #[arg(long)]
        keep_alive: bool,
        /// Working directory (defaults to the home directory).
        #[arg(long, value_name = "DIR")]
        cwd: Option<PathBuf>,
        /// Environment variable for the command. Repeatable. Never put secrets here.
        #[arg(long = "env", value_name = "KEY=VALUE")]
        environment: Vec<String>,
        /// Replace an existing job created by `rhythm add` under this name.
        #[arg(long)]
        replace: bool,
        #[arg(last = true, required = true, value_name = "COMMAND")]
        command: Vec<String>,
    },
    /// Record the runs of an existing job without changing anything else about it.
    Adopt {
        job: String,
        /// Reload the job even if it is running now, which stops that run.
        #[arg(long)]
        restart: bool,
    },
    /// Stop recording a job's runs, restoring its original command.
    Unadopt {
        job: String,
        #[arg(long)]
        restart: bool,
    },
    /// Unload and delete a job created by `rhythm add`; its history is kept.
    Remove { job: String },
    /// Start a job now.
    Kick {
        job: String,
        /// Wait for the run to finish and report how it ended.
        #[arg(long)]
        wait: bool,
        /// Longest wait, in seconds.
        #[arg(long, default_value_t = 600, value_name = "SECONDS",
              value_parser = clap::value_parser!(u64).range(1..=86400))]
        timeout: u64,
    },
    /// Show Rhythm with disposable synthetic jobs, removed when the demo ends.
    Demo {
        /// Skip the pauses between steps.
        #[arg(long)]
        fast: bool,
    },
    #[command(name = "__drain", hide = true)]
    Drain {
        #[arg(long)]
        dir: Option<PathBuf>,
        #[arg(long)]
        forward: bool,
    },
    /// List a job's recorded runs, newest first.
    History {
        label: String,
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// Read another machine over ssh.
        #[arg(long)]
        host: Option<String>,
    },
    /// Print the output of a job's newest recorded run, or of `--run ID`.
    Logs {
        label: String,
        #[arg(long, value_name = "ID")]
        run: Option<i64>,
        /// Return at most this many of the newest bytes.
        #[arg(long, default_value_t = 64 * 1024)]
        bytes: u64,
        #[arg(long)]
        host: Option<String>,
    },
}

fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp
                    | clap::error::ErrorKind::DisplayVersion
                    | clap::error::ErrorKind::DisplayHelpOnMissingArgumentOrSubcommand
            ) =>
        {
            return if error.print().is_ok() {
                ExitCode::SUCCESS
            } else {
                ExitCode::FAILURE
            };
        }
        Err(_) => {
            return failure(
                std::env::args_os().any(|a| a == "--json"),
                Error::InvalidRequest,
            );
        }
    };
    let json = cli.json;
    match cli.command {
        Commands::Status { hosts } => status(json, cli.data_dir, &hosts),
        Commands::Attention {
            since_last_seen,
            consumer,
            hosts,
        } => attention(json, cli.data_dir, since_last_seen, &consumer, &hosts),
        Commands::Run {
            label,
            forward,
            command,
        } => {
            // An unrecordable run still runs: the recorder falls back to the bare command.
            let target = label.parse::<Label>().and_then(|label| {
                state_directory::root(cli.data_dir)
                    .and_then(|root| Ledger::open(root, ledger::RECORDER_WAIT))
                    .map(|ledger| (ledger, label))
            });
            recorder::record(
                target,
                Recording {
                    program: command,
                    forward,
                },
            )
        }
        Commands::Add {
            name,
            every,
            daily,
            weekly,
            at_load,
            keep_alive,
            cwd,
            environment,
            replace,
            command,
        } => {
            let result = (|| {
                let schedule = schedule_input::schedule(&ScheduleInput {
                    every: every.as_deref(),
                    daily: &daily,
                    weekly: &weekly,
                    at_load,
                    keep_alive,
                })?;
                let home = std::env::var_os("HOME")
                    .map(PathBuf::from)
                    .filter(|p| p.is_absolute())
                    .ok_or(Error::InvalidRequest)?;
                let current = std::env::current_dir().map_err(|_| Error::InvalidRequest)?;
                let search_path = std::env::var("PATH").ok();
                let caller = Caller {
                    home: &home,
                    current_directory: &current,
                    search_path: search_path.as_deref(),
                };
                let (label, added) = authoring::add(
                    &Launchd::current_user()?,
                    &recorder_for(cli.data_dir.clone())?,
                    &caller,
                    NewJob {
                        name,
                        schedule,
                        command,
                        working_directory: cwd,
                        environment,
                        replace,
                    },
                )?;
                Ok(serde_json::json!({
                    "type": "added",
                    "label": label.as_str(),
                    "replaced": added == Added::Replaced,
                }))
            })();
            respond(json, result, |added| {
                let label = added["label"].as_str().unwrap_or_default();
                let verb = if added["replaced"] == true {
                    "Updated"
                } else {
                    "Added"
                };
                format!(
                    "{verb} {label}. Start it now with `rhythm kick {}`.\n",
                    label.trim_start_matches(OWNED_PREFIX)
                )
            })
        }
        Commands::Adopt { job, restart } => {
            let result = authoring::resolve(&job).and_then(|label| {
                let adoption = authoring::adopt(
                    &Launchd::current_user()?,
                    &recorder_for(cli.data_dir.clone())?,
                    &label,
                    restart,
                )?;
                Ok(serde_json::json!({
                    "type": "adopted",
                    "label": label.as_str(),
                    "changed": adoption != Adoption::AlreadyRecorded,
                }))
            });
            respond(json, result, |adopted| match adopted["changed"].as_bool() {
                Some(true) => format!(
                    "{} now records its runs.\n",
                    adopted["label"].as_str().unwrap_or_default()
                ),
                _ => format!(
                    "{} already records its runs.\n",
                    adopted["label"].as_str().unwrap_or_default()
                ),
            })
        }
        Commands::Unadopt { job, restart } => {
            let result = authoring::resolve(&job).and_then(|label| {
                authoring::release(&Launchd::current_user()?, &label, restart)?;
                Ok(serde_json::json!({"type": "unadopted", "label": label.as_str()}))
            });
            respond(json, result, |released| {
                format!(
                    "{} runs its original command again; its history is kept.\n",
                    released["label"].as_str().unwrap_or_default()
                )
            })
        }
        Commands::Remove { job } => {
            let result = authoring::resolve(&job).and_then(|label| {
                authoring::remove(&Launchd::current_user()?, &label)?;
                Ok(serde_json::json!({"type": "removed", "label": label.as_str()}))
            });
            respond(json, result, |removed| {
                format!(
                    "Removed {}; its history is kept.\n",
                    removed["label"].as_str().unwrap_or_default()
                )
            })
        }
        Commands::Kick { job, wait, timeout } => {
            keep_recorder_reachable(cli.data_dir.clone());
            let result = authoring::resolve(&job).and_then(|label| {
                kick::kick(
                    &Launchd::current_user()?,
                    &label,
                    wait.then(|| Duration::from_secs(timeout)),
                    &ledgers(cli.data_dir.clone()),
                )
            });
            let failed =
                matches!(&result, Ok(KickResponse { finished: Some(f), .. }) if !f.succeeded());
            let code = respond(json, result, kick_render::render);
            if failed { ExitCode::FAILURE } else { code }
        }
        Commands::Demo { fast } => {
            let result = this_executable().and_then(|exe| rhythm::demo::run(exe, fast, color()));
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => failure(json, error),
            }
        }
        Commands::Drain { dir, forward } => {
            recorder::drain(dir, forward);
            ExitCode::SUCCESS
        }
        Commands::History { label, limit, host } => {
            let result = authoring::resolve(&label).and_then(|label| match remote(host)? {
                Some(host) => Ssh::system().call(
                    &host,
                    &["history", label.as_str(), "--limit", &limit.to_string()],
                ),
                None => run_history::history(local_ledger(cli.data_dir)?.as_ref(), &label, limit),
            });
            respond(json, result, |history| {
                history_render::render(history, Timestamp::now(), &chrono::Local)
            })
        }
        Commands::Logs {
            label,
            run,
            bytes,
            host,
        } => {
            let result = authoring::resolve(&label).and_then(|label| match remote(host)? {
                Some(host) => {
                    let mut args = vec![
                        "logs".to_owned(),
                        label.to_string(),
                        "--bytes".into(),
                        bytes.to_string(),
                    ];
                    if let Some(run) = run {
                        args.extend(["--run".into(), run.to_string()]);
                    }
                    let args: Vec<&str> = args.iter().map(String::as_str).collect();
                    Ssh::system().call(&host, &args)
                }
                None => run_history::logs(local_ledger(cli.data_dir)?.as_ref(), &label, run, bytes),
            });
            respond(json, result, |logs| {
                if logs.truncated {
                    eprintln!("rhythm: earlier output of run {} is not shown", logs.run.id);
                }
                logs.output.clone()
            })
        }
    }
}

fn status(json: bool, data_dir: Option<PathBuf>, hosts: &[String]) -> ExitCode {
    keep_recorder_reachable(data_dir.clone());
    let targets = match Target::list(hosts) {
        Ok(targets) => targets,
        Err(error) => return failure(json, error),
    };
    let response = status::gather(
        &targets,
        || status::jobs(&Launchd::current_user()?, &ledgers(data_dir.clone())),
        &Ssh::system(),
    );
    let complete = response
        .hosts
        .iter()
        .all(|h| matches!(h.outcome, HostOutcome::Jobs { .. }));
    let output = if json {
        serde_json::to_string_pretty(&response).unwrap() + "\n"
    } else {
        status_render::render(&response.hosts, Timestamp::now(), color())
    };
    match (write_output(&output), complete) {
        (ExitCode::SUCCESS, true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

fn attention(
    json: bool,
    data_dir: Option<PathBuf>,
    since_last_seen: bool,
    consumer: &str,
    hosts: &[String],
) -> ExitCode {
    let (targets, consumer) = match (Target::list(hosts), Consumer::parse(consumer)) {
        (Ok(targets), Ok(consumer)) => (targets, consumer),
        (Err(error), _) | (_, Err(error)) => return failure(json, error),
    };
    keep_recorder_reachable(data_dir.clone());
    let local = || {
        let store = match since_last_seen {
            true => Some(AttentionStore::open(&state_directory::root(
                data_dir.clone(),
            )?)?),
            false => None,
        };
        let previous = store.as_ref().map(|s| s.load(&consumer)).transpose()?;
        let (items, seen) = attention::look(
            &Launchd::current_user()?,
            &ledgers(data_dir.clone()),
            previous.as_ref(),
        )?;
        Ok((items, store.zip(seen)))
    };
    let ssh = Ssh::system();
    let remote = |host: &Host| {
        let mut args = vec!["attention"];
        if since_last_seen {
            args.extend(["--since-last-seen", "--as", consumer.as_str()]);
        }
        let response: AttentionResponse = ssh.call(host, &args)?;
        match response.hosts.into_iter().next().map(|h| h.outcome) {
            Some(AttentionOutcome::Items { items }) => Ok((items, None)),
            Some(AttentionOutcome::Failed { error }) => Err(error),
            None => Err(Error::RemoteIncompatible),
        }
    };
    let mut pending = None;
    let response = AttentionResponse {
        hosts: targets::each(&targets, local, remote)
            .into_iter()
            .map(|(host, result)| AttentionHost {
                host,
                outcome: match result {
                    Ok((items, seen)) => {
                        pending = pending.take().or(seen);
                        AttentionOutcome::Items { items }
                    }
                    Err(error) => AttentionOutcome::Failed { error },
                },
            })
            .collect(),
    };
    let complete = response
        .hosts
        .iter()
        .all(|h| matches!(h.outcome, AttentionOutcome::Items { .. }));
    let output = if json {
        serde_json::to_string_pretty(&response).unwrap() + "\n"
    } else {
        match attention_render::render(&response, Timestamp::now()) {
            // Hooks read silence as "nothing to say"; a person deserves an answer.
            empty if empty.is_empty() && unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1 => {
                "Nothing needs attention.\n".to_owned()
            }
            text => text,
        }
    };
    let written = write_output(&output);
    // Marked seen only once delivered, so an interrupted hook loses nothing.
    if written == ExitCode::SUCCESS
        && let Some((store, seen)) = pending
        && store.save(&consumer, &seen).is_err()
    {
        return failure(json, Error::StorageUnavailable);
    }
    match (written, complete) {
        (ExitCode::SUCCESS, true) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// Each recorded job's ledger. An unreadable ledger only hides run history.
fn ledgers(data_dir: Option<PathBuf>) -> impl Fn(&RunStore) -> Option<Ledger> {
    move |store| {
        let dir = match store {
            RunStore::Default => data_dir.clone(),
            RunStore::Directory(dir) => Some(dir.clone()),
        };
        local_ledger(dir).ok().flatten()
    }
}

/// Jobs reach the recorder through a stable link to this executable.
fn recorder_for(data_dir: Option<PathBuf>) -> operation_error::Result<Recorder> {
    let root = state_directory::root(data_dir.clone())?;
    Ok(Recorder {
        executable: recorder_link::point(&root, &this_executable()?)?,
        data_dir,
        diagnostics: root.path().join("recorder.log"),
    })
}

fn this_executable() -> operation_error::Result<PathBuf> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|_| Error::InvalidRequest)
}

/// After an upgrade removed the executable jobs reach the recorder through,
/// any command run from the new one restores that path.
fn keep_recorder_reachable(data_dir: Option<PathBuf>) {
    if let (Ok(Some(root)), Ok(executable)) =
        (state_directory::existing(data_dir), this_executable())
    {
        let _ = recorder_link::repair(&root, &executable);
    }
}

fn local_ledger(data_dir: Option<PathBuf>) -> operation_error::Result<Option<Ledger>> {
    match state_directory::existing(data_dir)? {
        Some(root) => Ledger::open_existing(root),
        None => Ok(None),
    }
}

fn remote(host: Option<String>) -> operation_error::Result<Option<Host>> {
    match host.as_deref() {
        None | Some(Target::LOCAL) => Ok(None),
        Some(host) => host.parse().map(Some),
    }
}

fn respond<T: Serialize>(
    json: bool,
    result: operation_error::Result<T>,
    text: impl FnOnce(&T) -> String,
) -> ExitCode {
    match result {
        Ok(value) if json => write_output(&(serde_json::to_string_pretty(&value).unwrap() + "\n")),
        Ok(value) => write_output(&text(&value)),
        Err(error) => failure(json, error),
    }
}

fn color() -> bool {
    std::env::var_os("NO_COLOR").is_none()
        && std::env::var_os("TERM").is_some_and(|t| t != "dumb")
        && unsafe { libc::isatty(libc::STDOUT_FILENO) } == 1
}

fn write_output(output: &str) -> ExitCode {
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => ExitCode::FAILURE,
    }
}

fn failure(json: bool, error: Error) -> ExitCode {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({"error":{"code":error,"message":error.message()}})
        );
    } else {
        eprintln!("{}", error.message());
    }
    ExitCode::FAILURE
}
