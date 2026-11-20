//! Read a regular scheduler definition without waiting on devices or pipes.
use std::{
    fs::File,
    io::{self, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

pub(crate) const MAX_BYTES: u64 = 1024 * 1024;

pub(crate) fn read(path: &Path) -> io::Result<Vec<u8>> {
    if !path.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let file = File::options()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(io::ErrorKind::InvalidData.into());
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(io::ErrorKind::InvalidData.into());
    }
    Ok(bytes)
}
