//! Deliver a notice without delaying the job: a desktop notification by default,
//! the operator's own command when the job's environment names one, or nothing.
use std::{
    os::unix::process::CommandExt,
    path::PathBuf,
    process::{Command, Stdio},
};

/// Set to `off` in a job's environment to silence its notices.
pub const SWITCH: &str = "RHYTHM_NOTIFY";
/// An executable receiving the title and message as its two arguments.
pub const COMMAND: &str = "RHYTHM_NOTIFY_COMMAND";

#[derive(Debug, PartialEq, Eq)]
pub enum Notifier {
    Off,
    Desktop,
    Command(PathBuf),
}
impl Notifier {
    pub fn from_environment() -> Self {
        if std::env::var_os(SWITCH).is_some_and(|v| v == "off") {
            return Self::Off;
        }
        match std::env::var_os(COMMAND).map(PathBuf::from) {
            Some(command) if command.is_absolute() => Self::Command(command),
            Some(_) => Self::Off,
            None if cfg!(target_os = "macos") => Self::Desktop,
            None => Self::Off,
        }
    }
    /// Starts delivery and returns at once; failures are not the job's concern.
    pub fn deliver(&self, title: &str, message: &str) {
        let mut command = match self {
            Self::Off => return,
            Self::Command(program) => {
                let mut command = Command::new(program);
                command.args([title, message]);
                command
            }
            Self::Desktop => {
                // Text travels as arguments, never as AppleScript source.
                let mut command = Command::new("/usr/bin/osascript");
                command.args([
                    "-e",
                    "on run argv",
                    "-e",
                    "display notification (item 2 of argv) with title (item 1 of argv)",
                    "-e",
                    "end run",
                    title,
                    message,
                ]);
                command
            }
        };
        // launchd stops the job's whole process group once the recorder
        // exits; delivery runs in its own session so it can finish.
        unsafe {
            command.pre_exec(|| {
                libc::setsid();
                Ok(())
            });
        }
        let _ = command
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
    }
}
