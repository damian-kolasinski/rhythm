//! Bounded output of one run. The newest bytes survive: when the current file
//! fills, it replaces the previous one, so each run keeps 512 KiB to 1 MiB.
use crate::{
    operation_error::{Error, Result},
    private_storage,
};
use std::{
    fs::{self, File, OpenOptions},
    io::{self, Read, Seek, SeekFrom, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

pub const SEGMENT: u64 = 512 * 1024;
const CURRENT: &str = "output.log";
const PREVIOUS: &str = "output.log.1";
const DROPPED: &str = "dropped";

pub struct OutputLog {
    dir: PathBuf,
    file: File,
    written: u64,
}
impl OutputLog {
    pub fn create(dir: &Path) -> Result<Self> {
        Ok(Self {
            file: create(&dir.join(CURRENT)).map_err(|_| Error::StorageUnavailable)?,
            dir: dir.to_owned(),
            written: 0,
        })
    }
    /// Continues an existing run's output, as the drainer does.
    pub fn append(dir: &Path) -> Result<Self> {
        let file = OpenOptions::new()
            .append(true)
            .create(true)
            .mode(0o600)
            .custom_flags(libc::O_NOFOLLOW)
            .open(dir.join(CURRENT))
            .map_err(|_| Error::StorageUnavailable)?;
        private_storage::validate_file(&file)?;
        Ok(Self {
            written: file
                .metadata()
                .map_err(|_| Error::StorageUnavailable)?
                .len(),
            file,
            dir: dir.to_owned(),
        })
    }
    pub fn write(&mut self, bytes: &[u8]) -> io::Result<()> {
        if self.written > 0 && self.written + bytes.len() as u64 > SEGMENT {
            let previous = self.dir.join(PREVIOUS);
            if previous.exists() {
                create(&self.dir.join(DROPPED))?;
            }
            fs::rename(self.dir.join(CURRENT), previous)?;
            self.file = create(&self.dir.join(CURRENT))?;
            self.written = 0;
        }
        self.file.write_all(bytes)?;
        self.written += bytes.len() as u64;
        Ok(())
    }
}

fn create(path: &Path) -> io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
        .mode(0o600)
        .custom_flags(libc::O_NOFOLLOW)
        .open(path)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Tail {
    pub bytes: Vec<u8>,
    /// Earlier output exists but is not included.
    pub truncated: bool,
}

/// The last `max` retained bytes of a run's output, oldest first.
pub fn tail(dir: &Path, max: u64) -> Result<Tail> {
    let mut parts = vec![];
    for name in [PREVIOUS, CURRENT] {
        match File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(dir.join(name))
        {
            Ok(file) => {
                private_storage::validate_file(&file)?;
                parts.push(file);
            }
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(_) => return Err(Error::StorageUnavailable),
        }
    }
    let sizes: Vec<u64> = parts
        .iter()
        .map(|f| f.metadata().map(|m| m.len()))
        .collect::<io::Result<_>>()
        .map_err(|_| Error::StorageUnavailable)?;
    let total: u64 = sizes.iter().sum();
    let mut skip = total.saturating_sub(max);
    let mut bytes = Vec::with_capacity(total.min(max) as usize);
    for (mut file, size) in parts.into_iter().zip(sizes) {
        let start = skip.min(size);
        skip -= start;
        file.seek(SeekFrom::Start(start))
            .and_then(|_| file.take(size - start).read_to_end(&mut bytes))
            .map_err(|_| Error::StorageUnavailable)?;
    }
    Ok(Tail {
        bytes,
        truncated: total > max || dir.join(DROPPED).exists(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::DirBuilderExt;

    fn private_dir() -> tempfile::TempDir {
        let temp = tempfile::TempDir::new().unwrap();
        fs::DirBuilder::new()
            .mode(0o700)
            .create(temp.path().join("run"))
            .unwrap();
        temp
    }

    #[test]
    fn keeps_short_output_whole() {
        let temp = private_dir();
        let dir = temp.path().join("run");
        let mut log = OutputLog::create(&dir).unwrap();
        log.write(b"hello ").unwrap();
        log.write(b"world").unwrap();
        assert_eq!(
            tail(&dir, 1024).unwrap(),
            Tail {
                bytes: b"hello world".to_vec(),
                truncated: false
            }
        );
        assert_eq!(
            tail(&dir, 5).unwrap(),
            Tail {
                bytes: b"world".to_vec(),
                truncated: true
            }
        );
    }

    #[test]
    fn rotation_keeps_the_newest_output_across_files() {
        let temp = private_dir();
        let dir = temp.path().join("run");
        let mut log = OutputLog::create(&dir).unwrap();
        let chunk = vec![b'a'; 4096];
        for _ in 0..(SEGMENT / 4096) {
            log.write(&chunk).unwrap();
        }
        log.write(b"tail").unwrap();
        let all = tail(&dir, u64::MAX).unwrap();
        assert_eq!(all.bytes.len() as u64, SEGMENT + 4);
        assert!(all.bytes.ends_with(b"atail"));
        assert!(!all.truncated);
        for _ in 0..(SEGMENT / 4096) {
            log.write(&chunk).unwrap();
        }
        let after_second_rotation = tail(&dir, u64::MAX).unwrap();
        assert!(after_second_rotation.truncated);
        assert!(after_second_rotation.bytes.len() as u64 <= 2 * SEGMENT);
        // The second rotation dropped the first segment, so `tail` now leads.
        assert!(after_second_rotation.bytes.starts_with(b"tail"));
    }

    #[test]
    fn a_run_without_output_files_is_empty() {
        let temp = private_dir();
        assert_eq!(
            tail(&temp.path().join("run"), 10).unwrap(),
            Tail {
                bytes: vec![],
                truncated: false
            }
        );
    }
}
