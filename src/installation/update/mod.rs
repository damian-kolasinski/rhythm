//! Discover signed releases and replace the companion app and CLI together.
mod catalog;
mod download;
mod records;
mod selection;
#[cfg(test)]
mod tests;

use std::{
    fs,
    os::unix::process::CommandExt,
    path::Path,
    process::Command,
    time::{Duration, Instant},
};

use semver::Version;
use serde::{Deserialize, Serialize};

use super::{
    Installed, Paths, REPOSITORY, install_locked, installed_version, mise::Mise, window::Desktop,
};
use crate::{
    VERSION, bounded_command,
    jobs::{Observation, Supervisor},
    operation_error::{Error, Result},
    runs::Timestamp,
};
pub use catalog::Catalog;

const CACHE: &str = "release-check.json";
const JOURNAL: &str = "release-update.json";
const HOUR_MS: i64 = 3_600_000;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Check {
    pub available: Option<Version>,
    pub offline: bool,
    checked_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Progress {
    pub version: Version,
    pub phase: Phase,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Phase {
    Downloading,
    WaitingForWork,
    Installing,
    SelectingCli,
    Reopening,
    Succeeded,
    Failed { reason: Error, rolled_back: bool },
}

impl Phase {
    pub fn terminal(self) -> bool {
        matches!(self, Self::Succeeded | Self::Failed { .. })
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Downloading => "Verifying the signed release…",
            Self::WaitingForWork => "Waiting for running routines…",
            Self::Installing => "Installing the app and CLI…",
            Self::SelectingCli => "Selecting the CLI in mise…",
            Self::Reopening => "Reopening Rhythm…",
            Self::Succeeded => "Rhythm is up to date.",
            Self::Failed { reason, .. } => reason.message(),
        }
    }
}

/// The normal hourly check also uses the saved answer at startup.
pub fn check(paths: &Paths, force: bool) -> Result<Check> {
    let repository = REPOSITORY.ok_or(Error::ReleaseUnavailable)?;
    let current = VERSION.parse().map_err(|_| Error::ReleaseUnavailable)?;
    check_with(
        paths,
        &catalog::Github(repository),
        force,
        Timestamp::now(),
        &current,
    )
}

fn check_with(
    paths: &Paths,
    catalog: &impl Catalog,
    force: bool,
    now: Timestamp,
    current: &Version,
) -> Result<Check> {
    paths.validate()?;
    let cache = paths.area.join(CACHE);
    let saved: Option<Check> = records::read(&cache)?;
    if let Some(check) = &saved
        && !force
        && (0..HOUR_MS).contains(&now.0.saturating_sub(check.checked_at))
    {
        return Ok(newer(check.clone(), current));
    }
    let (available, offline) = match catalog.latest() {
        Ok(version) => (version, false),
        Err(_) => (saved.and_then(|check| check.available), true),
    };
    let check = newer(
        Check {
            available,
            offline,
            checked_at: now.0,
        },
        current,
    );
    if paths.cli().is_file() {
        // A network lookup never holds the lifecycle lock. An uninstall can
        // finish while it is in flight; in that case there is nothing to cache.
        if let Ok(_guard) = paths.lock()
            && paths.cli().is_file()
        {
            records::write(&cache, &check)?;
        }
    }
    Ok(check)
}

fn newer(mut answer: Check, current: &Version) -> Check {
    answer.available = answer.available.filter(|version| {
        version.pre.is_empty()
            && version.build.is_empty()
            && version.cmp_precedence(current).is_gt()
    });
    answer
}

pub fn progress(paths: &Paths) -> Result<Option<Progress>> {
    paths.validate()?;
    records::read(&paths.area.join(JOURNAL))
}

/// An interrupted helper leaves the verified target in the journal. A retry
/// verifies that exact target again, even if one executable already reached it.
pub fn recovery(paths: &Paths) -> Result<Option<Version>> {
    if !paths.cli().is_file() || !paths.area.join(JOURNAL).is_file() {
        return Ok(None);
    }
    match paths.lock() {
        Ok(_guard) => interrupted(paths),
        Err(Error::InstallationBusy) => Ok(None),
        Err(error) => Err(error),
    }
}

fn interrupted(paths: &Paths) -> Result<Option<Version>> {
    let Some(job) = progress(paths)? else {
        return Ok(None);
    };
    let Some(cli) = installed_version(paths)?.and_then(|version| catalog::stable(&version)) else {
        return Ok(None);
    };
    if !job.version.pre.is_empty() || !job.version.build.is_empty() {
        return Ok(None);
    }
    let app = super::window::build(&paths.app)?;
    let mixed = app.is_some_and(|app| app.version != cli.to_string());
    if job.version >= cli
        && (!job.phase.terminal()
            || (matches!(job.phase, Phase::Failed { .. }) && job.version == cli && mixed))
    {
        return Ok(Some(job.version));
    }
    Ok(None)
}

pub fn install(
    paths: &Paths,
    desktop: &impl Desktop,
    supervisor: &impl Supervisor,
    open: bool,
    expected: Option<&str>,
) -> Result<Installed> {
    let repository = REPOSITORY.ok_or(Error::ReleaseUnavailable)?;
    let running: Version = VERSION.parse().map_err(|_| Error::ReleaseUnavailable)?;
    let available = match recovery(paths)? {
        Some(version) => Some(version),
        None => check(paths, false)?.available,
    }
    .ok_or(Error::ReleaseUnavailable)?;
    let _guard = paths.lock()?;
    let current: Version = installed_version(paths)?
        .ok_or(Error::InstallationFailed)?
        .parse()
        .map_err(|_| Error::InstallationFailed)?;
    let repair = interrupted(paths)?.as_ref() == Some(&available);
    if expected.is_some_and(|expected| available.to_string() != expected)
        || available < running
        || available < current
        || (!repair && (available == running || available == current))
    {
        return Err(Error::ReleaseUnavailable);
    }
    if open && !desktop.available() {
        return Err(Error::WindowUnavailable);
    }
    let mise = Mise::discover(&paths.home, repository).ok_or(Error::MiseUnavailable)?;
    run_install(
        paths, desktop, supervisor, &mise, repository, available, open,
    )
}

fn run_install(
    paths: &Paths,
    desktop: &impl Desktop,
    supervisor: &impl Supervisor,
    mise: &Mise,
    repository: &str,
    version: Version,
    open: bool,
) -> Result<Installed> {
    let record = |phase| {
        records::write(
            &paths.area.join(JOURNAL),
            &Progress {
                version: version.clone(),
                phase,
            },
        )
    };
    record(Phase::Downloading)?;
    let mut rollback_failed = false;
    let result = (|| {
        let stage = download::verify(paths, mise, repository, &version)?;
        record(Phase::WaitingForWork)?;
        wait_for_work(supervisor)?;
        let mut selection = selection::Selection::prepare(paths, mise, repository, &version)?;
        record(Phase::Installing)?;
        let installed = install_locked(
            paths,
            desktop,
            &stage.binary,
            &version.to_string(),
            open,
            true,
            || {
                if let Some(selection) = &mut selection {
                    record(Phase::SelectingCli)?;
                    selection.activate(paths, mise, stage.binary.parent().unwrap())?;
                }
                if open {
                    record(Phase::Reopening)?;
                }
                Ok(())
            },
        );
        if installed.is_err()
            && let Some(selection) = &mut selection
        {
            rollback_failed = selection.restore(paths, mise).is_err();
        }
        if selection.is_some_and(|selection| selection.linked) {
            let _ = stage.scratch.keep();
        }
        installed
    })();
    if let Err(reason) = result {
        let rolled_back = !rollback_failed && reason != Error::InstallationRollbackFailed;
        let reason = if rolled_back {
            reason
        } else {
            Error::InstallationRollbackFailed
        };
        record(Phase::Failed {
            reason,
            rolled_back,
        })?;
        return Err(reason);
    }
    // At this point the new window already acknowledged its frame. A full
    // journal disk must not make the already committed upgrade look failed.
    let _ = record(Phase::Succeeded);
    result
}

fn wait_for_work(supervisor: &impl Supervisor) -> Result<()> {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        if !supervisor.recorded_jobs()?.iter().any(|job| {
            matches!(job.observation, Observation::Loaded { pid: Some(_), .. })
                && job
                    .activation
                    .as_ref()
                    .is_some_and(|activation| !activation.is_service())
        }) {
            return Ok(());
        }
        if Instant::now() >= deadline {
            return Err(Error::UpdateDeferred);
        }
        std::thread::sleep(Duration::from_millis(350));
    }
}

/// A detached CLI survives the old application's exit. It uses the same
/// transaction as an operator-initiated update.
pub fn upgrade_window(cli: &Path, expected: Option<&Version>) -> Result<()> {
    let mut command = Command::new(cli);
    command.arg("--json");
    if let Some(version) = expected {
        command.args([
            "update",
            "install",
            "--open",
            "--expected",
            &version.to_string(),
        ]);
    } else {
        command.arg("ui");
    }
    // SAFETY: in the child, only the async-signal-safe setsid syscall runs before exec.
    unsafe {
        command.pre_exec(|| {
            (libc::setsid() >= 0)
                .then_some(())
                .ok_or_else(std::io::Error::last_os_error)
        });
    }
    let output = bounded_command::run(command, Duration::from_secs(15 * 60), 65536)
        .map_err(|_| Error::InstallationFailed)?;
    if output.status.success() {
        Ok(())
    } else {
        #[derive(Deserialize)]
        struct Response {
            error: Failure,
        }
        #[derive(Deserialize)]
        struct Failure {
            code: Error,
        }
        Err(serde_json::from_slice::<Response>(&output.stderr)
            .map_or(Error::InstallationFailed, |response| response.error.code))
    }
}

pub(super) fn clear_records(paths: &Paths) -> Result<()> {
    for name in [CACHE, JOURNAL] {
        match fs::remove_file(paths.area.join(name)) {
            Ok(()) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::InstallationFailed),
        }
    }
    Ok(())
}

pub(super) fn clear_unused_downloads(paths: &Paths) {
    let Some(mise) = REPOSITORY.and_then(|repository| Mise::discover(&paths.home, repository))
    else {
        return;
    };
    let Ok(referenced) = mise.installed_roots(&paths.home) else {
        return;
    };
    let Ok(entries) = fs::read_dir(&paths.area) else {
        return;
    };
    for entry in entries.flatten().filter(|entry| {
        entry
            .file_name()
            .as_encoded_bytes()
            .starts_with(b"download-")
    }) {
        if entry.file_type().is_ok_and(|kind| kind.is_dir())
            && entry.path().join("mise.toml").is_file()
        {
            let directory = entry.path();
            if !referenced
                .iter()
                .any(|reference| reference.starts_with(&directory))
            {
                let _ = fs::remove_dir_all(directory);
            }
        }
    }
}
