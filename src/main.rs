use clap::{Parser, Subcommand};
use rhythm::{
    agent_setup::{self, Agent, Scope},
    agent_setup_render,
    attention::{self, AttentionHost, AttentionOutcome, AttentionResponse, Consumer},
    attention_render,
    attention_store::AttentionStore,
    authoring::{self, Added, Adoption, Caller, NewJob, OWNED_PREFIX, Recorder},
    findings_render, history_render,
    jobs::Label,
    kick::{self, KickResponse},
    kick_render,
    launchd::Launchd,
    ledger::{self, Ledger},
    open_findings,
    operation_error::{self, Error},
    recorder::{self, Recording},
    recorder_link,
    remote::{Host, Ssh},
    run_history,
    runs::Timestamp,
    schedule_input::{self, ScheduleInput},
    session_start, skill_guide, state_directory,
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
enum SkillCommand {
    /// Install the skill and session hook into the agents on this machine, or
    /// those named with --agent; again, update what an older Rhythm installed
    Setup {
        /// Only AGENT; repeat for more [default: every agent found]
        #[arg(long = "agent", value_enum, value_name = "AGENT")]
        agents: Vec<AgentArg>,
        /// Report what would change without changing anything
        #[arg(long)]
        dry_run: bool,
        /// Replace a skill named rhythm that Rhythm didn't write
        #[arg(long)]
        force: bool,
    },
    /// Report whether each agent has the skill and session hook
    Status,
    /// Remove the skill and session hook, leaving everything Rhythm didn't install
    Remove {
        /// Only AGENT; repeat for more [default: every agent Rhythm knows]
        #[arg(long = "agent", value_enum, value_name = "AGENT")]
        agents: Vec<AgentArg>,
        #[arg(long)]
        dry_run: bool,
    },
    /// Print the guide agents follow, or the reference NAME
    Show {
        /// One of the references the guide names, as in authoring
        name: Option<String>,
    },
    /// What the session hook tells a new session of AGENT
    #[command(hide = true)]
    SessionStart {
        #[arg(long, value_enum)]
        agent: AgentArg,
    },
}

#[derive(Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
enum AgentArg {
    Claude,
    Codex,
    #[value(name = "opencode")]
    OpenCode,
}
impl From<AgentArg> for Agent {
    fn from(arg: AgentArg) -> Self {
        match arg {
            AgentArg::Claude => Agent::Claude,
            AgentArg::Codex => Agent::Codex,
            AgentArg::OpenCode => Agent::OpenCode,
        }
    }
}

#[derive(Subcommand)]
enum Commands {
    /// Check and install a newer signed Rhythm release.
    Update {
        #[command(subcommand)]
        command: UpdateCommand,
    },
    /// Install this release's CLI and native app for the current user.
    Install {
        /// Open the installed app and wait until its window is ready.
        #[arg(long)]
        open: bool,
    },
    /// Open the installed Rhythm window.
    Ui,
    /// Remove the CLI and app, keeping routine definitions and recorded history.
    Uninstall {
        /// Leave the global mise request and downloaded release in place.
        #[arg(long)]
        keep_mise: bool,
    },
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
    /// Set coding agents up to use Rhythm, and print the guide they follow.
    Skill {
        #[command(subcommand)]
        command: SkillCommand,
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
    /// List a job's recorded runs, newest first, with consecutive skipped runs on one line.
    History {
        label: String,
        /// How many runs that were not skipped; the skipped runs among them come along.
        #[arg(long, default_value_t = 20)]
        limit: u32,
        /// List every skipped run on its own line.
        #[arg(long)]
        all: bool,
        /// Read another machine over ssh.
        #[arg(long)]
        host: Option<String>,
    },
    /// List the problems a job reports as open now, from its newest result that listed them.
    Findings {
        label: String,
        /// Read another machine over ssh.
        #[arg(long)]
        host: Option<String>,
    },
    /// Print the output of a job's newest recorded run that was not skipped, or of `--run ID`.
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

#[derive(Subcommand)]
enum UpdateCommand {
    /// Check for a newer stable release now.
    Check,
    /// Show the last update's state.
    Status,
    /// Verify and install the available release.
    Install {
        /// Open the new window and wait for it before confirming the upgrade.
        #[arg(long)]
        open: bool,
        #[arg(long, hide = true)]
        expected: Option<String>,
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
        #[cfg(target_os = "macos")]
        Commands::Update { command } => {
            use rhythm::installation::{Paths, update, window::MacDesktop};
            let paths = match Paths::current() {
                Ok(paths) => paths,
                Err(error) => return failure(json, error),
            };
            match command {
                UpdateCommand::Check => {
                    respond(json, update::check(&paths, true), |check| {
                        match (&check.available, check.offline) {
                            (Some(version), false) => format!(
                                "Rhythm {version} is available. Run rhythm update install.\n"
                            ),
                            (Some(version), true) => format!(
                                "Rhythm {version} was available at the last successful check.\n"
                            ),
                            (None, false) => "Rhythm is up to date.\n".into(),
                            (None, true) => "GitHub is unreachable. Try again later.\n".into(),
                        }
                    })
                }
                UpdateCommand::Status => respond(json, update::progress(&paths), |progress| {
                    progress.as_ref().map_or_else(
                        || "No Rhythm update has been started.\n".into(),
                        |job| format!("Rhythm {} · {}\n", job.version, job.phase.label()),
                    )
                }),
                UpdateCommand::Install { open, expected } => respond(
                    json,
                    Launchd::current_user().and_then(|launchd| {
                        update::install(&paths, &MacDesktop, &launchd, open, expected.as_deref())
                    }),
                    |result| format!("Rhythm {} installed.\n", result.version),
                ),
            }
        }
        #[cfg(target_os = "macos")]
        Commands::Install { open } => {
            use rhythm::installation::{self, Paths, WindowState, window::MacDesktop};
            let installed = Paths::current().and_then(|paths| {
                installation::install(&paths, &MacDesktop, &this_executable()?, open)
            });
            let opened = installed
                .as_ref()
                .is_ok_and(|result| result.window == WindowState::Opened);
            let code = respond(json, installed, |result| {
                let mut report = format!(
                    "Rhythm {} installed. Desktop window: {:?}.\n",
                    result.version, result.window
                );
                if let Some(error) = result.window_error {
                    report.push_str(error.message());
                    report.push('\n');
                }
                report
            });
            if open && !opened {
                ExitCode::FAILURE
            } else {
                code
            }
        }
        #[cfg(target_os = "macos")]
        Commands::Ui => {
            use rhythm::installation::{self, Paths, window::MacDesktop};
            let opened = Paths::current().and_then(|paths| installation::open(&paths, &MacDesktop));
            respond(json, opened.map(|_| "opened"), |_| {
                "Rhythm window opened.\n".into()
            })
        }
        #[cfg(target_os = "macos")]
        Commands::Uninstall { keep_mise } => {
            use rhythm::installation::{self, Paths};
            let removed = Paths::current().and_then(|paths| {
                installation::uninstall(&paths, &Launchd::current_user()?, keep_mise)
            });
            let mise_failed = removed.as_ref().is_ok_and(|result| result.mise.failed());
            let code = respond(json, removed, |result| {
                format!(
                    "Rhythm installation removed. Routines and history kept. mise: {:?}.{}{}\n",
                    result.mise,
                    if result.link_conflict {
                        " An unrelated CLI link was kept."
                    } else {
                        ""
                    },
                    if result.mise.failed() {
                        " Run mise unuse --global for the Rhythm packslip request."
                    } else {
                        ""
                    },
                )
            });
            if mise_failed { ExitCode::FAILURE } else { code }
        }
        #[cfg(not(target_os = "macos"))]
        Commands::Install { .. }
        | Commands::Ui
        | Commands::Uninstall { .. }
        | Commands::Update { .. } => failure(json, Error::UnsupportedPlatform),
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
            // Marks the job for what its command runs, recorded or not.
            // SAFETY: nothing else runs yet.
            unsafe { std::env::set_var(recorder::JOB, &label) };
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
                let _guard = authoring_guard()?;
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
                let _guard = authoring_guard()?;
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
            keep_current(cli.data_dir.clone());
            let result = authoring::resolve(&job).and_then(|label| {
                kick::kick(
                    &Launchd::current_user()?,
                    &label,
                    wait.then(|| Duration::from_secs(timeout)),
                    &status::local_ledgers(cli.data_dir.clone()),
                )
            });
            let failed =
                matches!(&result, Ok(KickResponse { finished: Some(f), .. }) if !f.succeeded());
            let code = respond(json, result, kick_render::render);
            if failed { ExitCode::FAILURE } else { code }
        }
        Commands::Skill { command } => skill(json, cli.data_dir, command),
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
        Commands::History {
            label,
            limit,
            all,
            host,
        } => {
            let result = authoring::resolve(&label).and_then(|label| match remote(host)? {
                Some(host) => Ssh::system().call(
                    &host,
                    &["history", label.as_str(), "--limit", &limit.to_string()],
                ),
                None => run_history::history(local_ledger(cli.data_dir)?.as_ref(), &label, limit),
            });
            respond(json, result, |history| {
                let skips = if all {
                    history_render::Skips::List
                } else {
                    history_render::Skips::Collapse
                };
                history_render::render(history, Timestamp::now(), &chrono::Local, skips)
            })
        }
        Commands::Findings { label, host } => {
            let result = authoring::resolve(&label).and_then(|label| match remote(host)? {
                Some(host) => Ssh::system().call(&host, &["findings", label.as_str()]),
                None => open_findings::findings(local_ledger(cli.data_dir)?.as_ref(), &label),
            });
            respond(json, result, |findings| {
                findings_render::render(findings, Timestamp::now())
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
    keep_current(data_dir.clone());
    let targets = match Target::list(hosts) {
        Ok(targets) => targets,
        Err(error) => return failure(json, error),
    };
    let response = status::gather(
        &targets,
        || {
            status::jobs(
                &Launchd::current_user()?,
                &status::local_ledgers(data_dir.clone()),
            )
        },
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
    keep_current(data_dir.clone());
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
            &status::local_ledgers(data_dir.clone()),
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

fn skill(json: bool, data_dir: Option<PathBuf>, command: SkillCommand) -> ExitCode {
    // Plain text whatever --json says, and never a failure: agents add it to
    // the session as it is.
    if let SkillCommand::SessionStart { agent } = command {
        return session_start(agent.into(), data_dir);
    }
    let Some(home) = home() else {
        return failure(json, Error::InvalidRequest);
    };
    let scope = Scope::user(home.clone());
    let agents = |named: Vec<AgentArg>, default: Vec<Agent>| match named.is_empty() {
        true => default,
        false => named.into_iter().map(Agent::from).collect(),
    };
    let (steps, dry_run) = match command {
        SkillCommand::Setup {
            agents: named,
            dry_run,
            force,
        } => (
            agent_setup::setup(&scope, &agents(named, scope.found()), force, dry_run),
            dry_run,
        ),
        SkillCommand::Remove {
            agents: named,
            dry_run,
        } => (
            agent_setup::remove(&scope, &agents(named, Agent::ALL.to_vec()), dry_run),
            dry_run,
        ),
        SkillCommand::Status => {
            let agents = agent_setup::status(&scope);
            let output = match json {
                true => to_json(&serde_json::json!({"type": "skill_status", "agents": agents})),
                false => agent_setup_render::status(&agents),
            };
            return write_output(&output);
        }
        SkillCommand::Show { name } => {
            return match skill_guide::show(name.as_deref()) {
                Some(text) if json => write_output(&to_json(
                    &serde_json::json!({"type": "skill_guide", "name": name, "text": text}),
                )),
                Some(text) => write_output(&text),
                None => failure(json, Error::NotFound),
            };
        }
        SkillCommand::SessionStart { .. } => unreachable!("handled above"),
    };
    let approvals: Vec<&str> = steps.iter().filter_map(|s| s.approval()).collect();
    let output = match json {
        true => to_json(&serde_json::json!({
            "type": "skill_setup",
            "dry_run": dry_run,
            "steps": steps,
            "approvals": approvals,
        })),
        false if dry_run => {
            "Dry run: nothing was changed.\n".to_owned() + &agent_setup_render::steps(&steps, &home)
        }
        false => agent_setup_render::steps(&steps, &home),
    };
    match (
        write_output(&output),
        steps.iter().any(|s| s.change.failed()),
    ) {
        (ExitCode::SUCCESS, false) => ExitCode::SUCCESS,
        _ => ExitCode::FAILURE,
    }
}

/// Never fails or slows a session: anything that goes wrong, or takes too
/// long, prints nothing.
fn session_start(agent: Agent, data_dir: Option<PathBuf>) -> ExitCode {
    // An agent run inside a job, as a scheduled `claude -p`, must not take
    // the news from the operator's next interactive session.
    if std::env::var_os(recorder::JOB).is_some() {
        return ExitCode::SUCCESS;
    }
    let (sender, received) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        if let Some(home) = home() {
            agent_setup::refresh(&Scope::user(home));
        }
        let context = (|| {
            let store = AttentionStore::open(&state_directory::root(data_dir.clone())?)?;
            let context = session_start::context(
                agent,
                &Launchd::current_user()?,
                &status::local_ledgers(data_dir),
                &store,
                Timestamp::now(),
            )?;
            Ok::<_, Error>((store, context))
        })();
        let _ = sender.send(context);
    });
    if let Ok(Ok((store, context))) = received.recv_timeout(SESSION_START_DEADLINE)
        && write_output(&context.text) == ExitCode::SUCCESS
        && let Ok(consumer) = Consumer::parse(agent.id())
    {
        let _ = store.save(&consumer, &context.seen);
    }
    ExitCode::SUCCESS
}

/// Agents give the hook five seconds.
const SESSION_START_DEADLINE: Duration = Duration::from_secs(3);

fn home() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
}

fn to_json(value: &serde_json::Value) -> String {
    serde_json::to_string_pretty(value).unwrap() + "\n"
}

/// Each recorded job's ledger. An unreadable ledger only hides run history.
/// Jobs reach the recorder through a stable link to this executable.
fn recorder_for(data_dir: Option<PathBuf>) -> operation_error::Result<Recorder> {
    let root = state_directory::root(data_dir.clone())?;
    Ok(Recorder {
        executable: recorder_link::point(&root, &recorder_executable()?)?,
        data_dir,
        diagnostics: root.path().join("recorder.log"),
    })
}

fn authoring_guard() -> operation_error::Result<Option<std::fs::File>> {
    #[cfg(target_os = "macos")]
    {
        rhythm::installation::Paths::current()?.lock().map(Some)
    }
    #[cfg(not(target_os = "macos"))]
    Ok(None)
}

fn recorder_executable() -> operation_error::Result<PathBuf> {
    #[cfg(target_os = "macos")]
    {
        use rhythm::installation::{self, Paths};
        let paths = Paths::current()?;
        if installation::installed_version(&paths)?.is_some() {
            return Ok(paths.cli());
        }
    }
    this_executable()
}

fn this_executable() -> operation_error::Result<PathBuf> {
    std::env::current_exe()
        .and_then(|p| p.canonicalize())
        .map_err(|_| Error::InvalidRequest)
}

/// After an upgrade removed the executable jobs reach the recorder through,
/// any command run from the new one restores that path.
fn keep_current(data_dir: Option<PathBuf>) {
    if let Ok(Some(root)) = state_directory::existing(data_dir)
        && recorder_link::location(&root).is_symlink()
        && !recorder_link::location(&root).exists()
        && let Ok(_guard) = authoring_guard()
        && let Ok(executable) = recorder_executable()
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
