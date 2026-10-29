use clap::{Parser, Subcommand};
use rhythm::{
    launchd::Launchd,
    operation_error::{self, Error},
    remote::Ssh,
    status::{self, HostOutcome, Target},
    status_render,
};
use std::{
    io::{self, Write},
    process::ExitCode,
};

#[derive(Parser)]
#[command(version, about)]
struct Cli {
    /// Machine-readable output; diagnostics are structured and source values are omitted.
    #[arg(long, global = true)]
    json: bool,
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
    match cli.command {
        Commands::Status { hosts } => {
            let targets = match targets(&hosts) {
                Ok(targets) => targets,
                Err(error) => return failure(cli.json, error),
            };
            let response = status::gather(
                &targets,
                || status::jobs(&Launchd::current_user()?),
                &Ssh::system(),
            );
            let complete = response
                .hosts
                .iter()
                .all(|h| matches!(h.outcome, HostOutcome::Jobs { .. }));
            let output = if cli.json {
                serde_json::to_string_pretty(&response).unwrap() + "\n"
            } else {
                status_render::render(&response.hosts, color())
            };
            match (write_output(&output), complete) {
                (ExitCode::SUCCESS, true) => ExitCode::SUCCESS,
                _ => ExitCode::FAILURE,
            }
        }
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
