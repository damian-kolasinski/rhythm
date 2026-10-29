//! Replace a definition file and reload its job, restoring the previous state
//! when launchd refuses the new one.
use super::launchctl::{Domain, Launchctl};
use crate::operation_error::{Error, Result};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
};

/// Readable by launchd and the owner's tools, as definitions usually are.
pub const NEW_MODE: u32 = 0o644;

pub struct Change<'a> {
    pub path: &'a Path,
    pub label: &'a str,
    pub new: &'a [u8],
    /// The definition being replaced; `None` creates the file, which must not exist.
    pub old: Option<&'a [u8]>,
    /// Where the job is loaded now.
    pub loaded: Option<Domain>,
    /// Where to load the new definition; `None` leaves it unloaded.
    pub load: Option<Domain>,
}

pub fn apply(launchctl: &Launchctl, change: Change) -> Result<()> {
    let mode = match change.old {
        // A private definition stays private.
        Some(_) => fs::symlink_metadata(change.path)
            .map(|m| m.permissions().mode() & 0o7777)
            .map_err(|_| Error::DefinitionsUnavailable)?,
        None => NEW_MODE,
    };
    write(change.path, change.new, mode, change.old.is_none())?;
    if let Some(domain) = change.loaded
        && let Err(error) = launchctl.bootout(domain, change.label)
    {
        restore(change.path, change.old, mode)?;
        return Err(error);
    }
    let Some(domain) = change.load else {
        return Ok(());
    };
    let Err(error) = launchctl.bootstrap(domain, change.path) else {
        return Ok(());
    };
    restore(change.path, change.old, mode).map_err(|_| Error::RollbackIncomplete)?;
    if let (Some(domain), Some(_)) = (change.loaded, change.old) {
        launchctl
            .bootstrap(domain, change.path)
            .map_err(|_| Error::RollbackIncomplete)?;
    }
    Err(error)
}

fn restore(path: &Path, old: Option<&[u8]>, mode: u32) -> Result<()> {
    match old {
        Some(old) => write(path, old, mode, false),
        None => fs::remove_file(path).map_err(|_| Error::DefinitionsUnavailable),
    }
}

/// Atomic replacement through a hidden sibling, which inventory ignores. With
/// `exclusive`, an existing file at `path` is never replaced.
fn write(path: &Path, bytes: &[u8], mode: u32, exclusive: bool) -> Result<()> {
    let parent = path.parent().ok_or(Error::InvalidRequest)?;
    fs::create_dir_all(parent).map_err(|_| Error::DefinitionsUnavailable)?;
    let temp = parent.join(format!(
        ".{}.{}.tmp",
        path.file_name().unwrap().to_string_lossy(),
        std::process::id()
    ));
    let result = (|| {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(mode)
            .open(&temp)?;
        // The process umask must not narrow or widen the intended mode.
        file.set_permissions(fs::Permissions::from_mode(mode))?;
        file.write_all(bytes)?;
        file.sync_all()?;
        if exclusive {
            fs::hard_link(&temp, path)?;
            fs::remove_file(&temp)
        } else {
            fs::rename(&temp, path)
        }
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&temp);
        return Err(if error.kind() == std::io::ErrorKind::AlreadyExists {
            Error::AlreadyExists
        } else {
            Error::DefinitionsUnavailable
        });
    }
    Ok(())
}
