//! Per-user desktop installation. Callers hold the lifecycle lock across all
//! changes to the installed executables, bundle, and release selection.
mod mise;
mod paths;
mod swap;
#[cfg(test)]
mod tests;
pub mod window;

use std::{
    fs,
    io::{Read, Write},
    os::unix::fs::{PermissionsExt, symlink},
    path::Path,
    process::Command,
    time::Duration,
};

use serde::Serialize;

use crate::{
    VERSION, authoring, bounded_command,
    jobs::{Label, Observation, Supervisor},
    operation_error::{Error, Result},
    private_storage::{self, Root},
    recorder_link,
    wrapper_command::Wrapped,
};
pub use paths::Paths;
use paths::owned;
use swap::Swap;
use window::Desktop;

pub const REPOSITORY: Option<&str> = option_env!("RHYTHM_RELEASE_REPOSITORY");
const RECEIPT: &str = "rhythm-installation";

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum WindowState {
    Opened,
    Placed,
    BackgroundSession,
    NotIncluded,
    Failed,
}

#[derive(Debug, Serialize)]
pub struct Installed {
    pub version: String,
    pub window: WindowState,
}

#[derive(Debug, Serialize)]
pub struct Uninstalled {
    pub routines_released: usize,
    pub link_conflict: bool,
    pub kept: &'static [&'static str],
    pub mise: mise::State,
}

pub fn install(
    paths: &Paths,
    desktop: &impl Desktop,
    source: &Path,
    open: bool,
) -> Result<Installed> {
    let _guard = paths.lock()?;
    install_locked(paths, desktop, source, VERSION, open, false, || Ok(()))
}

pub(crate) fn install_locked(
    paths: &Paths,
    desktop: &impl Desktop,
    source: &Path,
    version: &str,
    open: bool,
    transactional: bool,
    finish: impl FnOnce() -> Result<()>,
) -> Result<Installed> {
    let root = Root::open(paths.state.clone())?;
    let recorder = recorder_link::location(&root);
    match owned(&recorder)? {
        Some(metadata) if !metadata.is_symlink() => return Err(Error::InstallationConflict),
        Some(_) => private_storage::private_dir(recorder.parent().unwrap())?,
        None => {}
    }
    let old_recorder = fs::read_link(&recorder).ok();
    let cli = paths.cli();
    let current = cli.parent().unwrap();
    let has_cli = validate_current(current)?;
    let has_link = validate_link(&paths.link, &cli)?;
    let has_app = window::build(&paths.app)?.is_some();
    let packaged_ui = source.with_file_name("rhythm-ui");
    let has_ui = fs::symlink_metadata(&packaged_ui).is_ok_and(|metadata| metadata.is_file());
    if transactional && !has_ui {
        return Err(Error::InstallationFailed);
    }
    let clock = tempfile::Builder::new()
        .prefix("cli-")
        .tempdir_in(&paths.area)
        .map_err(|_| Error::InstallationFailed)?;
    let pair = clock.path().join("current");
    fs::create_dir(&pair).map_err(|_| Error::InstallationFailed)?;
    copy_executable(source, &pair.join("rhythm"))?;
    if has_ui {
        copy_executable(&packaged_ui, &pair.join("rhythm-ui"))?;
    }
    fs::write(pair.join(RECEIPT), version).map_err(|_| Error::InstallationFailed)?;
    let cli_swap = Swap::new(clock, "current", current.to_owned(), has_cli);
    let mut state = if !desktop.available() {
        WindowState::BackgroundSession
    } else if !has_ui {
        WindowState::NotIncluded
    } else {
        WindowState::Placed
    };
    let app = if state == WindowState::Placed {
        match stage_app(paths, desktop, &packaged_ui, version, has_app) {
            Ok(app) => Some(app),
            Err(_) if !transactional => {
                state = WindowState::Failed;
                None
            }
            Err(error) => return Err(error),
        }
    } else {
        None
    };
    let link_parent = paths.link.parent().unwrap();
    fs::create_dir_all(link_parent).map_err(|_| Error::InstallationFailed)?;
    let mut app_applied = false;
    let mut linked = false;
    let mut repointed = false;
    cli_swap.apply()?;
    let result = (|| {
        let mut verify = Command::new(&cli);
        verify.arg("--version");
        let verified = bounded_command::run(verify, Duration::from_secs(5), 4096)
            .ok()
            .filter(|output| output.status.success())
            .ok_or(Error::InstallationFailed)?;
        if verified.stdout != format!("rhythm {version}\n").as_bytes() {
            return Err(Error::InstallationFailed);
        }
        if !has_link {
            symlink(&cli, &paths.link).map_err(|_| Error::InstallationFailed)?;
            linked = true;
        }
        if old_recorder.as_ref().is_some_and(|target| target != &cli) {
            recorder_link::point(&root, &cli)?;
            repointed = true;
        }
        if let Some(app) = &app {
            match app.apply() {
                Ok(()) => app_applied = true,
                Err(_) if !transactional => state = WindowState::Failed,
                Err(error) => return Err(error),
            }
        }
        if open && app_applied {
            match desktop.open(&paths.app, &paths.home) {
                Ok(()) => state = WindowState::Opened,
                Err(_) if !transactional => state = WindowState::Failed,
                Err(error) => return Err(error),
            }
        }
        finish()
    })();
    if let Err(error) = result {
        let app_restored = !app_applied || app.unwrap().restore().is_ok();
        let link_restored = !linked || fs::remove_file(&paths.link).is_ok();
        let recorder_restored =
            !repointed || recorder_link::point(&root, old_recorder.as_ref().unwrap()).is_ok();
        let cli_restored = cli_swap.restore().is_ok();
        return if app_restored && link_restored && recorder_restored && cli_restored {
            Err(error)
        } else {
            Err(Error::InstallationRollbackFailed)
        };
    }
    Ok(Installed {
        version: version.into(),
        window: state,
    })
}

fn stage_app(
    paths: &Paths,
    desktop: &impl Desktop,
    source: &Path,
    version: &str,
    replacing: bool,
) -> Result<Swap> {
    let parent = paths.app.parent().unwrap();
    fs::create_dir_all(parent).map_err(|_| Error::InstallationFailed)?;
    let stage = tempfile::Builder::new()
        .prefix(".rhythm-app-")
        .tempdir_in(parent)
        .map_err(|_| Error::InstallationFailed)?;
    let app = stage.path().join("Rhythm.app");
    window::write(&app, source, version)?;
    desktop.sign(&app)?;
    Ok(Swap::new(stage, "Rhythm.app", paths.app.clone(), replacing))
}

pub fn open(paths: &Paths, desktop: &impl Desktop) -> Result<()> {
    let _guard = paths.lock()?;
    if window::build(&paths.app)?.is_none() {
        return Err(Error::WindowUnavailable);
    }
    desktop.open(&paths.app, &paths.home)
}

pub fn uninstall(
    paths: &Paths,
    supervisor: &impl Supervisor,
    keep_mise: bool,
) -> Result<Uninstalled> {
    let _guard = paths.lock()?;
    let cli = paths.cli();
    let current = cli.parent().unwrap();
    let installed = validate_current(current)?;
    let app = window::build(&paths.app)?.is_some();
    let mut link_conflict = false;
    let link = match validate_link(&paths.link, &cli) {
        Err(Error::InstallationConflict) => {
            link_conflict = true;
            false
        }
        result => result?,
    };
    let root = Root::open(paths.state.clone())?;
    let recorder = recorder_link::location(&root);
    let own_recorder = fs::read_link(&recorder).is_ok_and(|target| target == cli);
    let mut routines_released = 0;
    if installed {
        let mut to_release = Vec::new();
        for job in supervisor.recorded_jobs()? {
            let label: Label = job.label.parse()?;
            let program = supervisor.program(&label)?;
            let Some(wrapped) = Wrapped::parse(&program.arguments) else {
                continue;
            };
            if wrapped.executable == cli
                || wrapped.executable == paths.link
                || (own_recorder && wrapped.executable == recorder)
                || wrapped
                    .executable
                    .canonicalize()
                    .is_ok_and(|resolved| resolved == cli)
            {
                if matches!(job.observation, Observation::Loaded { pid: Some(_), .. }) {
                    return Err(Error::RestartRequired);
                }
                to_release.push(label);
            }
        }
        for label in to_release {
            authoring::release(supervisor, &label, false)?;
            routines_released += 1;
        }
    }
    if link {
        fs::remove_file(&paths.link).map_err(|_| Error::InstallationFailed)?;
    }
    if own_recorder {
        fs::remove_file(recorder).map_err(|_| Error::InstallationFailed)?;
    }
    if app {
        fs::remove_dir_all(&paths.app).map_err(|_| Error::InstallationFailed)?;
    }
    if installed {
        fs::remove_dir_all(current).map_err(|_| Error::InstallationFailed)?;
    }
    Ok(Uninstalled {
        routines_released,
        link_conflict,
        kept: &["routine_definitions", "recorded_history"],
        mise: mise::forget(paths, keep_mise),
    })
}

fn validate_link(link: &Path, cli: &Path) -> Result<bool> {
    match owned(link)? {
        None => Ok(false),
        Some(metadata)
            if metadata.is_symlink() && fs::read_link(link).as_deref().ok() == Some(cli) =>
        {
            Ok(true)
        }
        _ => Err(Error::InstallationConflict),
    }
}

fn validate_current(current: &Path) -> Result<bool> {
    match owned(current)? {
        None => Ok(false),
        Some(metadata) if metadata.is_dir() => {
            let proof = owned(&current.join(RECEIPT))?;
            let cli = owned(&current.join("rhythm"))?;
            let entries = fs::read_dir(current).map_err(|_| Error::InstallationConflict)?;
            let unexpected = entries.into_iter().any(|entry| {
                entry.ok().is_none_or(|entry| {
                    !matches!(
                        entry.file_name().to_str(),
                        Some(RECEIPT | "rhythm" | "rhythm-ui")
                    )
                })
            });
            if !unexpected
                && proof.is_some_and(|metadata| metadata.is_file())
                && cli.is_some_and(|metadata| metadata.is_file())
            {
                Ok(true)
            } else {
                Err(Error::InstallationConflict)
            }
        }
        _ => Err(Error::InstallationConflict),
    }
}

pub(super) fn copy_executable(source: &Path, target: &Path) -> Result<()> {
    let mut input = fs::File::open(source).map_err(|_| Error::InstallationFailed)?;
    let metadata = input.metadata().map_err(|_| Error::InstallationFailed)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::InstallationFailed);
    }
    let mut output = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(target)
        .map_err(|_| Error::InstallationFailed)?;
    std::io::copy(
        &mut Read::by_ref(&mut input).take(metadata.len()),
        &mut output,
    )
    .and_then(|bytes| {
        if bytes == metadata.len() {
            output.flush()
        } else {
            Err(std::io::ErrorKind::UnexpectedEof.into())
        }
    })
    .and_then(|_| output.set_permissions(fs::Permissions::from_mode(0o755)))
    .map_err(|_| Error::InstallationFailed)
}
