use clap::{Parser, Subcommand};
use rhythm::{
    history_render,
    jobs::{Label, RunStore},
    launchd::Launchd,
    ledger::{self, Ledger},
    operation_error::{self, Error},
    recorder::{self, Recording},
    remote::{Host, Ssh},
    run_history,
    runs::Timestamp,
    state_directory,
    status::{self, HostOutcome, Target},
    status_render,
};
use serde::Serialize;
use std::{
    ffi::OsString,
    io::{self, Write},
    path::PathBuf,
    process::ExitCode,
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
        Commands::Drain { dir, forward } => {
            recorder::drain(dir, forward);
            ExitCode::SUCCESS
        }
        Commands::History { label, limit, host } => {
            let result = label
                .parse::<Label>()
                .and_then(|label| match remote(host)? {
                    Some(host) => Ssh::system().call(
                        &host,
                        &["history", label.as_str(), "--limit", &limit.to_string()],
                    ),
                    None => {
                        run_history::history(local_ledger(cli.data_dir)?.as_ref(), &label, limit)
                    }
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
            let result = label
                .parse::<Label>()
                .and_then(|label| match remote(host)? {
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
                    None => {
                        run_history::logs(local_ledger(cli.data_dir)?.as_ref(), &label, run, bytes)
                    }
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
    let targets = match targets(hosts) {
        Ok(targets) => targets,
        Err(error) => return failure(json, error),
    };
    let response = status::gather(
        &targets,
        || {
            // Inventory comes first: an unreadable ledger only hides run history.
            status::jobs(&Launchd::current_user()?, &|store: &RunStore| {
                let dir = match store {
                    RunStore::Default => data_dir.clone(),
                    RunStore::Directory(dir) => Some(dir.clone()),
                };
                local_ledger(dir).ok().flatten()
            })
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

fn targets(hosts: &[String]) -> operation_error::Result<Vec<Target>> {
    let mut targets: Vec<Target> = vec![];
    for host in hosts {
        let target = Target::parse(host)?;
        if !targets.contains(&target) {
            targets.push(target);
        }
    }
    if targets.is_empty() {
        targets.push(Target::Local);
    }
    Ok(targets)
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
