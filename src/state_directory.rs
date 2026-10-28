//! Resolve the private runtime directory at the CLI boundary.
use crate::{
    operation_error::{Error, Result},
    private_storage::Root,
};
use std::path::PathBuf;

pub fn root(data_dir: Option<PathBuf>) -> Result<Root> {
    let path = match data_dir {
        Some(path) => path,
        None => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or(Error::InvalidRequest)?;
            if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Rhythm")
            } else {
                home.join(".local/state/rhythm")
            }
        }
    };
    Root::open(path)
}
