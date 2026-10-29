//! A stable path to Rhythm for job definitions. Installs are versioned and an
//! upgrade can remove the executable a job names, so jobs name
//! `<state>/bin/rhythm` instead: a link to the executable that last created or
//! adopted a job, repaired by any later command that finds it dangling.
use crate::{
    operation_error::{Error, Result},
    private_storage::{self, Root},
};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn location(root: &Root) -> PathBuf {
    root.path().join("bin").join("rhythm")
}

/// Points the link at `executable`, which must be absolute.
pub fn point(root: &Root, executable: &Path) -> Result<PathBuf> {
    let link = location(root);
    if !executable.is_absolute() {
        return Err(Error::InvalidRequest);
    }
    if fs::read_link(&link).is_ok_and(|target| target == executable) {
        return Ok(link);
    }
    private_storage::private_dir(link.parent().unwrap())?;
    let temp = link.with_file_name(format!(".rhythm.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temp);
    std::os::unix::fs::symlink(executable, &temp)
        .and_then(|_| fs::rename(&temp, &link))
        .map_err(|_| {
            let _ = fs::remove_file(&temp);
            Error::StorageUnavailable
        })?;
    Ok(link)
}

/// Repoints a link whose target no longer exists; a working link is kept.
pub fn repair(root: &Root, executable: &Path) -> Result<()> {
    let link = location(root);
    if fs::symlink_metadata(&link).is_ok() && fs::metadata(&link).is_err() {
        point(root, executable)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn points_repairs_and_keeps_a_working_link() {
        let temp = tempfile::TempDir::new().unwrap();
        let root = Root::open(temp.path().join("state")).unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::write(&first, "").unwrap();
        fs::write(&second, "").unwrap();
        let link = point(&root, &first).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), first);
        repair(&root, &second).unwrap();
        assert_eq!(
            fs::read_link(&link).unwrap(),
            first,
            "a working link was moved"
        );
        fs::remove_file(&first).unwrap();
        repair(&root, &second).unwrap();
        assert_eq!(fs::read_link(&link).unwrap(), second);
        assert!(point(&root, Path::new("relative")).is_err());
    }
}
