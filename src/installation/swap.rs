use std::{
    ffi::CString,
    fs,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

use tempfile::TempDir;

use crate::operation_error::{Error, Result};

/// The old inode stays mapped in running processes and is held in a private
/// staging directory until every participant has finished.
pub(super) struct Swap {
    stage: Option<TempDir>,
    item: PathBuf,
    live: PathBuf,
    replaces: bool,
}

impl Swap {
    pub fn new(stage: TempDir, name: &str, live: PathBuf, replaces: bool) -> Self {
        Self {
            item: stage.path().join(name),
            stage: Some(stage),
            live,
            replaces,
        }
    }

    pub fn apply(&self) -> Result<()> {
        if self.replaces {
            exchange(&self.item, &self.live)
        } else {
            fs::rename(&self.item, &self.live).map_err(|_| Error::InstallationFailed)
        }
    }

    pub fn restore(mut self) -> Result<()> {
        let result = if self.replaces {
            exchange(&self.item, &self.live)
        } else {
            fs::rename(&self.live, &self.item).map_err(|_| Error::InstallationFailed)
        };
        if result.is_err() {
            let _ = self.stage.take().unwrap().keep();
            return Err(Error::InstallationRollbackFailed);
        }
        Ok(())
    }
}

fn exchange(first: &Path, second: &Path) -> Result<()> {
    let first = CString::new(first.as_os_str().as_bytes()).map_err(|_| Error::InvalidRequest)?;
    let second = CString::new(second.as_os_str().as_bytes()).map_err(|_| Error::InvalidRequest)?;
    // Both paths are on the same volume and the staging parent is private.
    if unsafe { libc::renamex_np(first.as_ptr(), second.as_ptr(), libc::RENAME_SWAP) } == 0 {
        Ok(())
    } else {
        Err(Error::InstallationFailed)
    }
}
