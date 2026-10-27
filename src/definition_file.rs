//! Bounded nonblocking reads for untrusted definition files.
use crate::operation_error::{Error, Result};
use serde::de::DeserializeOwned;
use std::{io::Read, os::unix::fs::OpenOptionsExt, path::Path};
pub fn read<T: DeserializeOwned>(path: &Path) -> Result<T> {
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Error::InvalidRequest)?;
    if !file
        .metadata()
        .map_err(|_| Error::InvalidRequest)?
        .is_file()
    {
        return Err(Error::InvalidRequest);
    }
    let mut bytes = vec![];
    file.take(131073)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidRequest)?;
    if bytes.len() > 131072 {
        return Err(Error::InvalidRequest);
    }
    serde_json::from_slice(&bytes).map_err(|_| Error::InvalidRequest)
}
