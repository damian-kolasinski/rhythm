//! Translate lifecycle operations to non-interactive, bounded mise commands.
use std::{
    ffi::OsStr,
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

use serde::{Deserialize, Serialize};

use super::{Paths, REPOSITORY};
use crate::{
    bounded_command,
    operation_error::{Error, Result},
};

#[derive(Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum State {
    Removed { version: String, pruned: bool },
    Absent,
    NoMise,
    Kept,
    Failed,
}

impl State {
    pub fn failed(&self) -> bool {
        matches!(self, Self::Failed | Self::Removed { pruned: false, .. })
    }
}

#[derive(Deserialize)]
pub(super) struct Request {
    pub version: String,
    pub source: Source,
}

#[derive(Deserialize)]
pub(super) struct Source {
    pub path: PathBuf,
}

pub(super) struct Mise {
    executable: PathBuf,
    pub tool: String,
}

impl Mise {
    pub fn discover(home: &Path, repository: &str) -> Option<Self> {
        fn executable(path: &Path) -> bool {
            path.is_absolute()
                && fs::metadata(path).is_ok_and(|metadata| {
                    metadata.is_file() && metadata.permissions().mode() & 0o111 != 0
                })
        }
        let paths = std::env::var_os("PATH")
            .map(|path| {
                std::env::split_paths(&path)
                    .filter(|dir| dir.is_absolute())
                    .map(|dir| dir.join("mise"))
                    .collect::<Vec<_>>()
            })
            .unwrap_or_default();
        let executable = if let Some(path) = std::env::var_os("RHYTHM_MISE") {
            let path = PathBuf::from(path);
            executable(&path).then_some(path)?
        } else {
            [
                home.join(".local/bin/mise"),
                home.join(".local/share/mise/bin/mise"),
                "/opt/homebrew/bin/mise".into(),
                "/usr/local/bin/mise".into(),
            ]
            .into_iter()
            .chain(paths)
            .find(|path| executable(path))?
        };
        Some(Self {
            executable,
            tool: format!("packslip:github.com/{repository}"),
        })
    }

    pub fn command(&self, home: &Path) -> Command {
        let mut command = Command::new(&self.executable);
        command
            .current_dir(home)
            .env("HOME", home)
            .env("MISE_YES", "1");
        command
    }

    pub fn run(
        &self,
        home: &Path,
        args: impl IntoIterator<Item = impl AsRef<OsStr>>,
    ) -> Result<Vec<u8>> {
        let mut command = self.command(home);
        command.args(args);
        run(command)
    }

    pub fn request(&self, home: &Path) -> Result<Option<Request>> {
        let bytes = self.run(home, ["ls", "--global", "--current", "--json", &self.tool])?;
        let mut entries: Vec<Request> =
            serde_json::from_slice(&bytes).map_err(|_| Error::MiseUnavailable)?;
        if entries.len() > 1 {
            return Err(Error::MiseUnavailable);
        }
        Ok(entries.pop())
    }
}

pub(super) fn run(command: Command) -> Result<Vec<u8>> {
    bounded_command::run(command, Duration::from_secs(180), 1024 * 1024)
        .ok()
        .filter(|output| output.status.success())
        .map(|output| output.stdout)
        .ok_or(Error::MiseUnavailable)
}

pub(super) fn forget(paths: &Paths, keep: bool) -> State {
    if keep {
        return State::Kept;
    }
    let Some(repository) = REPOSITORY else {
        return State::Absent;
    };
    let Some(mise) = Mise::discover(&paths.home, repository) else {
        return State::NoMise;
    };
    let Ok(backends) = mise.run(&paths.home, ["backends", "ls"]) else {
        return State::Failed;
    };
    if !String::from_utf8_lossy(&backends)
        .lines()
        .any(|line| line.trim() == "packslip")
    {
        return State::Absent;
    }
    let request = match mise.request(&paths.home) {
        Ok(None) => return State::Absent,
        Ok(Some(request)) => request,
        Err(_) => return State::Failed,
    };
    let config = request.source.path.as_os_str();
    if !config.as_encoded_bytes().starts_with(b"/")
        || mise
            .run(
                &paths.home,
                [
                    OsStr::new("unuse"),
                    OsStr::new("--no-prune"),
                    OsStr::new("--path"),
                    config,
                    OsStr::new(&mise.tool),
                ],
            )
            .is_err()
    {
        return State::Failed;
    }
    let pruned = mise
        .run(&paths.home, ["prune", "--yes", "--tools", &mise.tool])
        .is_ok();
    State::Removed {
        version: request.version,
        pruned,
    }
}
