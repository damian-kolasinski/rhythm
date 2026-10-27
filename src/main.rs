use clap::{Parser, Subcommand};
use rhythm::{discovery, launchd::LaunchdInventory, terminal};
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
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Inspect the current user's launchd registrations without changing them.
    Discover,
}

fn main() -> ExitCode {
    let cli = Cli::parse();
    if !cfg!(target_os = "macos") {
        return failure(
            cli.json,
            "unsupported_platform",
            "Discovery currently supports macOS only.",
        );
    }
    let Some(home) = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
    else {
        return failure(cli.json, "invalid_home", "HOME must be an absolute path.");
    };
    let inventory = discovery::discover(&LaunchdInventory::new(home.join("Library/LaunchAgents")));
    let output = if cli.json {
        serde_json::to_string_pretty(&inventory).expect("inventory serializes") + "\n"
    } else {
        terminal::render(&inventory)
    };
    match io::stdout().lock().write_all(output.as_bytes()) {
        Ok(()) => {
            if inventory.complete {
                ExitCode::SUCCESS
            } else {
                ExitCode::from(2)
            }
        }
        Err(e) if e.kind() == io::ErrorKind::BrokenPipe => ExitCode::SUCCESS,
        Err(_) => failure(
            cli.json,
            "output_unavailable",
            "Could not write the inventory.",
        ),
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
