use std::{
    fs,
    io::{Read, Write},
    path::Path,
};

use serde::{Serialize, de::DeserializeOwned};

use crate::{
    operation_error::{Error, Result},
    private_storage,
};

pub(super) fn read<T: DeserializeOwned>(path: &Path) -> Result<Option<T>> {
    let file = match fs::symlink_metadata(path) {
        Ok(_) => private_storage::private_file(path)?,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(_) => return Err(Error::StorageUnavailable),
    };
    let mut bytes = Vec::new();
    file.take(65537)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::StorageUnavailable)?;
    if bytes.len() > 65536 {
        return Err(Error::StorageUnavailable);
    }
    serde_json::from_slice(&bytes)
        .map(Some)
        .map_err(|_| Error::StorageUnavailable)
}

pub(super) fn write(path: &Path, value: &impl Serialize) -> Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(path.parent().unwrap())
        .map_err(|_| Error::StorageUnavailable)?;
    serde_json::to_writer(&mut temp, value).map_err(|_| Error::StorageUnavailable)?;
    temp.flush()
        .and_then(|_| temp.as_file().sync_all())
        .map_err(|_| Error::StorageUnavailable)?;
    temp.persist(path).map_err(|_| Error::StorageUnavailable)?;
    Ok(())
}
