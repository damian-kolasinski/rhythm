//! Resolve Rhythm's private state directory at the CLI boundary.
use crate::{
    operation_error::{Error, Result},
    private_storage::Root,
};
use std::path::PathBuf;

/// The directory, created if needed, for recording runs.
pub fn root(data_dir: Option<PathBuf>) -> Result<Root> {
    Root::open(path(data_dir)?)
}

/// The directory only if it exists: reading never creates state.
pub fn existing(data_dir: Option<PathBuf>) -> Result<Option<Root>> {
    let path = path(data_dir)?;
    match std::fs::symlink_metadata(&path) {
        Ok(_) => Root::open(path).map(Some),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(_) => Err(Error::StorageUnavailable),
    }
}

fn path(data_dir: Option<PathBuf>) -> Result<PathBuf> {
    match data_dir {
        Some(path) if path.is_absolute() => Ok(path),
        Some(_) => Err(Error::InvalidRequest),
        None => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or(Error::InvalidRequest)?;
            Ok(if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Rhythm")
            } else {
                home.join(".local/state/rhythm")
            })
        }
    }
}
