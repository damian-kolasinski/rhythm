//! Private filesystem boundary. Never echo paths or OS error messages.
use crate::operation_error::{Error, Result};
use std::{
    fs::{self, File, OpenOptions},
    os::{
        fd::AsRawFd,
        unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    },
    path::{Path, PathBuf},
};

#[derive(Clone)]
pub struct Root(PathBuf);
impl Root {
    pub fn open(path: PathBuf) -> Result<Self> {
        if !path.is_absolute() {
            return Err(Error::InvalidRequest);
        }
        match fs::symlink_metadata(&path) {
            Ok(m) => {
                if !m.is_dir() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
                    return Err(Error::InsecureStorage);
                }
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => fs::DirBuilder::new()
                .recursive(true)
                .mode(0o700)
                .create(&path)
                .map_err(|_| Error::StorageUnavailable)?,
            Err(_) => return Err(Error::StorageUnavailable),
        }
        validate_directory(&path)?;
        Ok(Self(path))
    }
    pub fn path(&self) -> &Path {
        &self.0
    }
}
pub fn private_dir(p: &Path) -> Result<()> {
    match fs::DirBuilder::new().mode(0o700).create(p) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(_) => return Err(Error::StorageUnavailable),
    }
    validate_directory(p)
}
fn validate_directory(p: &Path) -> Result<()> {
    let m = fs::symlink_metadata(p).map_err(|_| Error::StorageUnavailable)?;
    if !m.is_dir() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
        return Err(Error::InsecureStorage);
    }
    let file = OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NOFOLLOW | libc::O_DIRECTORY)
        .open(p)
        .map_err(|_| Error::InsecureStorage)?;
    reject_extended_acl(&file)
}
pub fn private_file(path: &Path) -> Result<File> {
    let f = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| Error::StorageUnavailable)?;
    validate_file(&f)?;
    Ok(f)
}
pub fn validate_file(f: &File) -> Result<()> {
    let m = f.metadata().map_err(|_| Error::StorageUnavailable)?;
    if !m.is_file() || m.uid() != unsafe { libc::getuid() } || m.mode() & 0o077 != 0 {
        return Err(Error::InsecureStorage);
    }
    reject_extended_acl(f)
}
#[cfg(target_os = "macos")]
fn reject_extended_acl(file: &File) -> Result<()> {
    use std::ffi::c_void;
    unsafe extern "C" {
        fn acl_get_fd_np(fd: i32, kind: u32) -> *mut c_void;
        fn acl_get_entry(acl: *mut c_void, index: i32, entry: *mut *mut c_void) -> i32;
        fn acl_free(acl: *mut c_void) -> i32;
    }
    // SAFETY: the descriptor is borrowed for this call; the allocated ACL is freed below.
    unsafe {
        let acl = acl_get_fd_np(file.as_raw_fd(), 0x100); // ACL_TYPE_EXTENDED
        if acl.is_null() {
            // Darwin reports ENOENT for an open inode with no extended ACL.
            return if std::io::Error::last_os_error().raw_os_error() == Some(libc::ENOENT) {
                Ok(())
            } else {
                Err(Error::InsecureStorage)
            };
        }
        let mut entry = std::ptr::null_mut();
        let found = acl_get_entry(acl, 0, &mut entry); // ACL_FIRST_ENTRY
        let error = std::io::Error::last_os_error().raw_os_error();
        acl_free(acl);
        if found == -1 && error == Some(libc::EINVAL) {
            Ok(())
        } else {
            Err(Error::InsecureStorage)
        }
    }
}
#[cfg(not(target_os = "macos"))]
fn reject_extended_acl(_file: &File) -> Result<()> {
    Ok(())
}
