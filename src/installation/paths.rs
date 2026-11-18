use std::{
    fs::{self, File},
    os::{fd::AsRawFd, unix::fs::MetadataExt},
    path::{Path, PathBuf},
};

use crate::{
    operation_error::{Error, Result},
    private_storage::{self, Root},
};

pub struct Paths {
    pub home: PathBuf,
    pub state: PathBuf,
    pub area: PathBuf,
    pub app: PathBuf,
    pub link: PathBuf,
}

impl Paths {
    pub fn current() -> Result<Self> {
        Self::new(
            std::env::var_os("HOME")
                .ok_or(Error::InvalidRequest)?
                .into(),
        )
    }

    pub fn new(home: PathBuf) -> Result<Self> {
        if !home.is_absolute() {
            return Err(Error::InvalidRequest);
        }
        directory(&home, false)?;
        let state = home.join("Library/Application Support/Rhythm");
        Ok(Self {
            app: home.join("Applications/Rhythm.app"),
            area: state.join("installation"),
            link: home.join(".local/bin/rhythm"),
            home,
            state,
        })
    }

    pub fn cli(&self) -> PathBuf {
        self.area.join("current/rhythm")
    }

    pub(super) fn previous_app(&self) -> PathBuf {
        self.app.with_file_name(".Rhythm-previous.app")
    }

    pub fn validate(&self) -> Result<()> {
        for dir in [
            self.home.join("Library"),
            self.home.join("Library/Application Support"),
            self.state.clone(),
            self.area.clone(),
            self.home.join(".local"),
            self.home.join(".local/bin"),
            self.home.join("Applications"),
        ] {
            directory(&dir, true)?;
        }
        Ok(())
    }

    pub fn lock(&self) -> Result<File> {
        self.validate()?;
        Root::open(self.state.clone())?;
        private_storage::private_dir(&self.area)?;
        let file = private_storage::private_file(&self.area.join("lock"))?;
        // An OS lock releases even if the process exits during a lifecycle operation.
        let locked = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if locked != 0 {
            return Err(Error::InstallationBusy);
        }
        Ok(file)
    }
}

pub(super) fn owned(path: &Path) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.uid() == unsafe { libc::getuid() } => Ok(Some(metadata)),
        Ok(_) => Err(Error::InstallationConflict),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Error::InstallationFailed),
    }
}

fn directory(path: &Path, missing: bool) -> Result<()> {
    match owned(path)? {
        Some(metadata) if metadata.is_dir() && metadata.mode() & 0o022 == 0 => Ok(()),
        None if missing => Ok(()),
        _ => Err(Error::InstallationConflict),
    }
}
