//! Each consumer's last look, kept privately next to the run ledger.
use crate::{
    attention::{Consumer, Snapshot},
    operation_error::{Error, Result},
    private_storage::{self, Root},
};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
};

const MAX_SNAPSHOT: u64 = 4 * 1024 * 1024;

pub struct AttentionStore {
    dir: PathBuf,
}
impl AttentionStore {
    pub fn open(root: &Root) -> Result<Self> {
        let dir = root.path().join("attention");
        private_storage::private_dir(&dir)?;
        Ok(Self { dir })
    }
    /// An unknown consumer has seen nothing yet.
    pub fn load(&self, consumer: &Consumer) -> Result<Snapshot> {
        let file = match File::options()
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_NONBLOCK)
            .open(self.path(consumer))
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Snapshot::default()),
            Err(_) => return Err(Error::StorageUnavailable),
        };
        private_storage::validate_file(&file)?;
        let mut bytes = vec![];
        file.take(MAX_SNAPSHOT + 1)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::StorageUnavailable)?;
        // A damaged snapshot only means everything is reported once more.
        Ok(serde_json::from_slice(&bytes).unwrap_or_default())
    }
    pub fn save(&self, consumer: &Consumer, snapshot: &Snapshot) -> Result<()> {
        let path = self.path(consumer);
        let temp = self
            .dir
            .join(format!(".{}.{}.tmp", consumer.as_str(), std::process::id()));
        let result = (|| -> Result<()> {
            let mut file = OpenOptions::new()
                .write(true)
                .create_new(true)
                .mode(0o600)
                .custom_flags(libc::O_NOFOLLOW)
                .open(&temp)
                .map_err(|_| Error::StorageUnavailable)?;
            private_storage::validate_file(&file)?;
            file.write_all(&serde_json::to_vec(snapshot).unwrap())
                .and_then(|_| file.sync_all())
                .and_then(|_| fs::rename(&temp, &path))
                .map_err(|_| Error::StorageUnavailable)
        })();
        if result.is_err() {
            let _ = fs::remove_file(&temp);
        }
        result
    }
    fn path(&self, consumer: &Consumer) -> PathBuf {
        self.dir.join(format!("{}.json", consumer.as_str()))
    }
}
