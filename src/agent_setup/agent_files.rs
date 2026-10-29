//! Read and replace files in agents' folders the way the operator keeps them:
//! through links into a dotfiles checkout, with their permissions, never
//! half-written, and never over a change another program made meanwhile.
use serde_json::Value;
use std::{
    fs,
    io::{self, Write},
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
};

/// A JSON file as it was read, to replace only if it is still that.
pub struct Read {
    /// `None` for a file that is missing or empty.
    pub value: Option<Value>,
    bytes: Option<Vec<u8>>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Replaced {
    Done,
    /// Another program changed the file after it was read; nothing was written.
    ChangedMeanwhile,
}

/// `Err` says why the file can't be used.
pub fn read_json(path: &Path) -> Result<Read, String> {
    let bytes = match fs::read(path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            return Ok(Read {
                value: None,
                bytes: None,
            });
        }
        Err(e) => return Err(format!("reading {}: {e}", path.display())),
    };
    let value = match String::from_utf8_lossy(&bytes).trim() {
        "" => None,
        text => Some(
            serde_json::from_str(text)
                .map_err(|_| format!("{} isn't valid JSON", path.display()))?,
        ),
    };
    Ok(Read {
        value,
        bytes: Some(bytes),
    })
}

/// Replaces the file `read` came from with `value`, unless it changed since.
pub fn replace_json(path: &Path, read: &Read, value: &Value) -> io::Result<Replaced> {
    let mut text = serde_json::to_string_pretty(value).map_err(io::Error::other)?;
    text.push('\n');
    replace(path, text.as_bytes(), Some(read))
}

/// Swaps `bytes` in for the file at `path`.
pub fn write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    replace(path, bytes, None).map(|_| ())
}

/// Through a link to the file if it is one; with its permissions, or the
/// usual ones for a new file. With `expected`, the file must still hold what
/// was read, checked just before the swap.
fn replace(path: &Path, bytes: &[u8], expected: Option<&Read>) -> io::Result<Replaced> {
    let path = through_links(path)?;
    let dir = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
    fs::create_dir_all(dir)?;
    let mode = fs::metadata(&path).map_or(0o644, |m| m.permissions().mode() & 0o7777);
    let mut file = tempfile::Builder::new()
        .prefix(".rhythm-")
        .tempfile_in(dir)?;
    file.write_all(bytes)?;
    file.as_file()
        .set_permissions(fs::Permissions::from_mode(mode))?;
    file.as_file().sync_all()?;
    if let Some(read) = expected {
        let now = match fs::read(&path) {
            Ok(bytes) => Some(bytes),
            Err(e) if e.kind() == io::ErrorKind::NotFound => None,
            Err(e) => return Err(e),
        };
        if now != read.bytes {
            return Ok(Replaced::ChangedMeanwhile);
        }
    }
    file.persist(&path).map_err(|e| e.error)?;
    Ok(Replaced::Done)
}

/// The file `path` names once links are followed; the last may not exist yet.
fn through_links(path: &Path) -> io::Result<PathBuf> {
    let mut path = path.to_owned();
    for _ in 0..40 {
        match fs::read_link(&path) {
            Ok(target) => {
                let dir = path.parent().ok_or(io::ErrorKind::InvalidInput)?;
                path = dir.join(target);
            }
            Err(e)
                if matches!(
                    e.kind(),
                    io::ErrorKind::InvalidInput | io::ErrorKind::NotFound
                ) =>
            {
                return Ok(path);
            }
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::other(format!(
        "{} links in a loop",
        path.display()
    )))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn writes_through_links_and_keeps_permissions() {
        let temp = tempfile::TempDir::new().unwrap();
        let real = temp.path().join("dotfiles/settings.json");
        fs::create_dir_all(real.parent().unwrap()).unwrap();
        fs::write(&real, "{\"a\": 1}").unwrap();
        fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
        let link = temp.path().join("settings.json");
        std::os::unix::fs::symlink("dotfiles/settings.json", &link).unwrap();
        let read = read_json(&link).unwrap();
        assert_eq!(
            replace_json(&link, &read, &json!({"a": 2})).unwrap(),
            Replaced::Done
        );
        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink()
        );
        assert_eq!(fs::read_to_string(&real).unwrap(), "{\n  \"a\": 2\n}\n");
        assert_eq!(
            fs::metadata(&real).unwrap().permissions().mode() & 0o777,
            0o600
        );
        // Nothing but the file itself is left in the folder.
        assert_eq!(fs::read_dir(real.parent().unwrap()).unwrap().count(), 1);
    }

    #[test]
    fn never_writes_over_a_change_made_meanwhile() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, "{\"a\": 1}").unwrap();
        let read = read_json(&path).unwrap();
        fs::write(&path, "{\"a\": 1, \"agent\": \"wrote this\"}").unwrap();
        assert_eq!(
            replace_json(&path, &read, &json!({"a": 2})).unwrap(),
            Replaced::ChangedMeanwhile
        );
        assert!(fs::read_to_string(&path).unwrap().contains("wrote this"));
        assert_eq!(fs::read_dir(temp.path()).unwrap().count(), 1);
    }

    #[test]
    fn keeps_numbers_as_written() {
        let temp = tempfile::TempDir::new().unwrap();
        let path = temp.path().join("settings.json");
        fs::write(&path, "{\"big\": 18446744073709551616, \"x\": 1.50}").unwrap();
        let read = read_json(&path).unwrap();
        replace_json(&path, &read, read.value.as_ref().unwrap()).unwrap();
        let text = fs::read_to_string(&path).unwrap();
        assert!(
            text.contains("18446744073709551616") && text.contains("1.50"),
            "{text}"
        );
    }

    #[test]
    fn reads_missing_and_empty_as_nothing_and_refuses_broken_json() {
        let temp = tempfile::TempDir::new().unwrap();
        assert!(
            read_json(&temp.path().join("missing.json"))
                .unwrap()
                .value
                .is_none()
        );
        fs::write(temp.path().join("empty.json"), "\n").unwrap();
        assert!(
            read_json(&temp.path().join("empty.json"))
                .unwrap()
                .value
                .is_none()
        );
        fs::write(temp.path().join("broken.json"), "{").unwrap();
        assert!(read_json(&temp.path().join("broken.json")).is_err());
    }
}
