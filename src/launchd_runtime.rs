//! User launchd adapter, limited to registrations created for a private Rhythm directory.
use crate::{private_storage, services::*};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, OpenOptions},
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
};

pub struct LaunchdRuntime {
    agents: PathBuf,
    launchctl: PathBuf,
    domain: String,
}
impl LaunchdRuntime {
    pub fn current_user() -> Result<Self> {
        if !cfg!(target_os = "macos") {
            return Err(Error::UnsupportedPlatform);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or(Error::InvalidRequest)?;
        Ok(Self {
            agents: home.join("Library/LaunchAgents"),
            launchctl: "/bin/launchctl".into(),
            domain: format!("gui/{}", unsafe { libc::getuid() }),
        })
    }
    pub fn at_boundary(agents: PathBuf, launchctl: PathBuf, domain: String) -> Self {
        Self {
            agents,
            launchctl,
            domain,
        }
    }
    fn command(&self, args: &[&std::ffi::OsStr]) -> Result<bool> {
        Command::new(&self.launchctl)
            .args(args)
            .output()
            .map(|o| o.status.success())
            .map_err(|_| Error::ExecutionUnavailable)
    }
    fn label(directory: &Path) -> String {
        format!(
            "dev.rhythm.runtime.{:x}",
            Sha256::digest(directory.as_os_str().as_encoded_bytes())
        )
    }
    fn location(&self, directory: &Path) -> (PathBuf, String) {
        let label = Self::label(directory);
        (
            self.agents.join(format!("{label}.plist")),
            format!("{}/{label}", self.domain),
        )
    }
    fn original(&self, path: &Path, directory: &Path) -> Result<Option<Vec<u8>>> {
        match fs::symlink_metadata(path) {
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(Error::StorageUnavailable),
            Ok(m) if !m.is_file() => return Err(Error::InsecureStorage),
            _ => {}
        }
        let file = private_storage::private_file(path)?;
        use std::io::Read;
        let mut bytes = vec![];
        file.take(131073)
            .read_to_end(&mut bytes)
            .map_err(|_| Error::StorageUnavailable)?;
        if bytes.len() > 131072 {
            return Err(Error::InsecureStorage);
        }
        let v = plist::Value::from_reader(std::io::Cursor::new(&bytes))
            .map_err(|_| Error::InsecureStorage)?;
        let d = v.as_dictionary().ok_or(Error::InsecureStorage)?;
        let args = d
            .get("ProgramArguments")
            .and_then(plist::Value::as_array)
            .ok_or(Error::InsecureStorage)?;
        let expected = directory.to_str().ok_or(Error::InvalidRequest)?;
        if d.get("Label").and_then(plist::Value::as_string) != Some(Self::label(directory).as_str())
            || args.len() != 5
            || args[1].as_string() != Some("--data-dir")
            || args[2].as_string() != Some(expected)
            || args[3].as_string() != Some("runtime")
            || args[4].as_string() != Some("run")
        {
            return Err(Error::InsecureStorage);
        }
        Ok(Some(bytes))
    }
    fn write(&self, path: &Path, bytes: &[u8]) -> Result<()> {
        let temp = path.with_extension(private_storage::random_hex()?);
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o600)
            .open(&temp)
            .map_err(|_| Error::StorageUnavailable)?;
        let result = (|| {
            private_storage::validate_file(&file)?;
            file.write_all(bytes)
                .map_err(|_| Error::StorageUnavailable)?;
            file.sync_all().map_err(|_| Error::StorageUnavailable)?;
            fs::rename(&temp, path).map_err(|_| Error::StorageUnavailable)
        })();
        if result.is_err() {
            let _ = fs::remove_file(temp);
        }
        result
    }
}
impl RuntimeSupervisor for LaunchdRuntime {
    fn install(&self, executable: &Path, directory: &Path) -> Result<()> {
        if !executable.is_absolute() || !directory.is_absolute() {
            return Err(Error::InvalidRequest);
        }
        if !self.command(&["print".as_ref(), self.domain.as_ref()])? {
            return Err(Error::ExecutionUnavailable);
        }
        fs::create_dir_all(&self.agents).map_err(|_| Error::StorageUnavailable)?;
        let (path, target) = self.location(directory);
        let original = self.original(&path, directory)?;
        let value = serde_json::json!({"Label":Self::label(directory),"ProgramArguments":[executable.to_str().ok_or(Error::InvalidRequest)?,"--data-dir",directory.to_str().ok_or(Error::InvalidRequest)?,"runtime","run"],"KeepAlive":true,"RunAtLoad":true,"ThrottleInterval":5,"ProcessType":"Background"});
        let mut bytes = vec![];
        plist::to_writer_xml(&mut bytes, &value).map_err(|_| Error::InvalidRequest)?;
        let loaded = self.command(&["print".as_ref(), target.as_ref()])?;
        if loaded && original.is_none() {
            return Err(Error::InsecureStorage);
        }
        if original.as_ref() == Some(&bytes) && loaded {
            return Ok(());
        }
        self.write(&path, &bytes)?;
        if loaded
            && !matches!(
                self.command(&["bootout".as_ref(), target.as_ref()]),
                Ok(true)
            )
        {
            if let Some(old) = original {
                self.write(&path, &old)?;
            } else {
                let _ = fs::remove_file(&path);
            }
            return Err(Error::ExecutionUnavailable);
        }
        if matches!(
            self.command(&["bootstrap".as_ref(), self.domain.as_ref(), path.as_os_str()]),
            Ok(true)
        ) {
            return Ok(());
        }
        if let Some(old) = original {
            self.write(&path, &old)?;
            let _ = self.command(&["bootstrap".as_ref(), self.domain.as_ref(), path.as_os_str()]);
        } else {
            let _ = fs::remove_file(path);
        }
        Err(Error::ExecutionUnavailable)
    }
    fn uninstall(&self, directory: &Path) -> Result<bool> {
        let (path, target) = self.location(directory);
        if self.original(&path, directory)?.is_none() {
            if self.command(&["print".as_ref(), target.as_ref()])? {
                return Err(Error::InsecureStorage);
            }
            return Ok(false);
        }
        let loaded = self.command(&["print".as_ref(), target.as_ref()])?;
        if loaded && !self.command(&["bootout".as_ref(), target.as_ref()])? {
            return Err(Error::ExecutionUnavailable);
        }
        fs::remove_file(path).map_err(|_| Error::StorageUnavailable)?;
        Ok(true)
    }
}
