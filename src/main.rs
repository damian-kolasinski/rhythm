use clap::{Parser, Subcommand};
use rhythm::{discovery, launchd::LaunchdInventory, terminal};
use rhythm::{
    service_cli::{self, RuntimeCommand, ServiceCommand},
    services,
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
    /// Inspect the current user's launchd registrations without changing them.
    Discover,
    /// Run or inspect the shared local runtime.
    Runtime {
        #[command(subcommand)]
        command: RuntimeCommand,
    },
    /// Create and control Rhythm-owned services.
    Service {
        #[command(subcommand)]
        command: ServiceCommand,
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
                let root = service_cli::root(cli.data_dir)?;
                let launch = rhythm::browser_launch::prepare(&root).await?;
                rhythm::browser_launch::deliver(&launch, cli.json, no_open)
            }
            .await;
            match result {
                Ok(()) => ExitCode::SUCCESS,
                Err(error) => failure(cli.json, &error.to_string(), &error.to_string()),
            }
        }
        Commands::Demo { no_open } => match rhythm::demo::run(cli.json, no_open).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => failure(cli.json, &error.to_string(), &error.to_string()),
        },
        Commands::Discover => discover(cli.json),
        Commands::Monitor { fd } => match rhythm::execution::run_monitor(fd).await {
            Ok(()) => ExitCode::SUCCESS,
            Err(_) => ExitCode::FAILURE,
        },
        Commands::Anchor => loop {
            std::thread::park();
        },
        command => {
            let result: services::Result<rhythm::runtime_protocol::Response> = async {
                let root = service_cli::root(cli.data_dir)?;
                match command {
                    Commands::Runtime { command } => service_cli::runtime(command, root).await,
                    Commands::Service { command } => service_cli::service(command, root).await,
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
                    false,
                ),
                Err(error) => failure(cli.json, &error.to_string(), &error.to_string()),
            }
        }
    }
}

fn discover(json: bool) -> ExitCode {
    if !cfg!(target_os = "macos") {
        return failure(
            json,
            "unsupported_platform",
            "Discovery currently supports macOS only.",
        );
    }
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    else {
        return failure(json, "invalid_home", "HOME must be an absolute path.");
    };
    let inventory = discovery::discover(&LaunchdInventory::new(home.join("Library/LaunchAgents")));
    let output = if json {
        serde_json::to_string_pretty(&inventory).expect("inventory serializes") + "\n"
    } else {
        terminal::render(&inventory)
    };
    write_output(&output, json, !inventory.complete)
}
fn write_output(output: &str, json: bool, partial: bool) -> ExitCode {
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => {
            if partial {
                ExitCode::from(2)
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => failure(json, "output_unavailable", "Could not write the inventory."),
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
