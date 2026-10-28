use clap::{Parser, Subcommand};
use rhythm::{
    operation_error,
    runtime_cli::{self, RuntimeCommand},
};
use std::{
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
    /// Private runtime storage directory (defaults to the user application state directory).
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Open the local web workspace.
    Ui {
        #[arg(long)]
        no_open: bool,
    },
    /// Run a disposable demo with synthetic work until interrupted.
    Demo {
        #[arg(long)]
        no_open: bool,
    },
    /// Run or inspect the local scheduler runtime.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    /// Create and control persisted interval and calendar schedules.
    Schedule {
        #[command(subcommand)]
        command: rhythm::schedule_cli::ScheduleCommand,
    },
    #[command(name = "__monitor", hide = true)]
    Monitor { fd: i32 },
    #[command(name = "__anchor", hide = true)]
    Anchor,
}

#[tokio::main(flavor = "current_thread")]
async fn main() -> ExitCode {
    let cli = match Cli::try_parse() {
        Ok(cli) => cli,
        Err(error)
            if matches!(
                error.kind(),
                clap::error::ErrorKind::DisplayHelp | clap::error::ErrorKind::DisplayVersion
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
                "invalid_request",
                "Invalid command. Run rhythm --help.",
            );
        }
    };
    match cli.command {
        Commands::Ui { no_open } => {
            let result = async {
                let root = rhythm::state_directory::root(cli.data_dir)?;
                let launch = rhythm::browser_launch::prepare(&root).await?;
                rhythm::browser_launch::deliver(&launch, cli.json, no_open)
            }
            .await;
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => operation_failure(cli.json, error),
            }
        }
        Commands::Demo { no_open } => match rhythm::demo::run(cli.json, no_open).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => operation_failure(cli.json, error),
        },
        Commands::Monitor { fd } => match rhythm::execution::run_monitor(fd).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Commands::Anchor => loop {
            std::thread::park();
        },
        command => {
            let result: operation_error::Result<rhythm::runtime_protocol::Response> = async {
                let root = rhythm::state_directory::root(cli.data_dir)?;
                match command {
                    Commands::Runtime { command } => runtime_cli::runtime(command, root).await,
                    Commands::Schedule { command } => {
                        rhythm::schedule_cli::run(command, root).await
                    }
                    _ => unreachable!(),
                }
            }
            .await;
            match result {
                Ok(response) => write_output(
                    &(serde_json::to_string_pretty(&response).unwrap() + "\n"),
                    cli.json,
                ),
                Err(error) => operation_failure(cli.json, error),
            }
        }
    }
}

fn operation_failure(json: bool, error: operation_error::Error) -> ExitCode {
    let code = error.to_string();
    let message = if error == operation_error::Error::UnsupportedSchema {
        "Unsupported database schema. Start with a fresh --data-dir."
    } else {
        &code
    };
    failure(json, &code, message)
}

fn write_output(output: &str, json: bool) -> ExitCode {
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => ExitCode::SUCCESS,
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => failure(json, "output_unavailable", "Could not write the response."),
    }
}

fn failure(json: bool, code: &str, message: &str) -> ExitCode {
    if json {
        eprintln!(
            "{}",
            serde_json::json!({"error":{"code":code,"message":message}})
        );
    } else {
        eprintln!("{message}");
    }
    ExitCode::FAILURE
}
