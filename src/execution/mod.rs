//! Execution-host adapter: a monitor retains the execution lock and a stdin lease.
mod monitor;
use crate::{
    private_storage::{self, Root},
    services::*,
};
pub use monitor::run_monitor;
use serde::{Deserialize, Serialize};
use std::{
    collections::BTreeMap,
    io::{Read, Write},
    os::{
        fd::{AsRawFd, FromRawFd, OwnedFd},
        unix::process::CommandExt,
    },
    path::PathBuf,
    process::{Child, ChildStdin, Command, Stdio},
};

#[derive(Serialize, Deserialize)]
struct Input {
    root: PathBuf,
    id: ExecutionId,
    definition: Definition,
}
struct Running {
    child: Child,
    lease: Option<ChildStdin>,
}
pub struct ProcessHost {
    root: Root,
    executable: PathBuf,
    runs: BTreeMap<ExecutionId, Running>,
}
impl ProcessHost {
    pub fn new(root: Root, executable: PathBuf) -> Self {
        Self {
            root,
            executable,
            runs: BTreeMap::new(),
        }
    }
    fn completion(&self, id: ExecutionId) -> Result<Completion> {
        private_storage::read_json(&self.root.run(id)?.join("result.json"))
    }
}
impl ExecutionHost for ProcessHost {
    fn start(&mut self, id: ExecutionId, definition: &Definition) -> Result<u32> {
        let frame = serde_json::to_vec(&Input {
            root: self.root.path().to_owned(),
            id,
            definition: definition.clone(),
        })
        .map_err(|_| Error::InvalidRequest)?;
        if frame.len() > 131072 {
            return Err(Error::ExecutionUnavailable);
        }
        let dir = self.root.run(id)?;
        let lock = private_storage::try_lock(&dir.join("monitor.lock"))?
            .ok_or(Error::ExecutionUnavailable)?;
        // Pass a dedicated descriptor through exec, preserving the lock throughout startup.
        let fd = unsafe { libc::fcntl(lock.as_raw_fd(), libc::F_DUPFD_CLOEXEC, 10) };
        if fd < 0 {
            return Err(Error::ExecutionUnavailable);
        }
        let descriptor = unsafe { OwnedFd::from_raw_fd(fd) };
        let mut command = Command::new(&self.executable);
        command
            .args(["__monitor", &fd.to_string()])
            .env_clear()
            .env("PATH", "/usr/bin:/bin:/usr/sbin:/sbin")
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .process_group(0);
        unsafe {
            command.pre_exec(move || {
                if libc::fcntl(fd, libc::F_SETFD, 0) < 0 {
                    Err(std::io::Error::last_os_error())
                } else {
                    Ok(())
                }
            });
        }
        private_storage::write_json(&dir.join("launch-attempt.json"), &true)?;
        let mut child = command.spawn().map_err(|_| Error::ExecutionUnavailable)?;
        drop(descriptor);
        drop(lock);
        let mut lease = child.stdin.take().unwrap();
        let write = lease
            .write_all(&(frame.len() as u64).to_be_bytes())
            .and_then(|()| lease.write_all(&frame));
        if write.is_err() {
            drop(lease);
            let _ = child.wait();
            return Err(Error::ExecutionUnavailable);
        }
        let pid = child.id();
        self.runs.insert(
            id,
            Running {
                child,
                lease: Some(lease),
            },
        );
        Ok(pid)
    }
    fn stop(&mut self, id: ExecutionId) -> Result<()> {
        if let Some(run) = self.runs.get_mut(&id)
            && let Some(mut lease) = run.lease.take()
        {
            let _ = lease.write_all(b"S");
        }
        Ok(())
    }
    fn poll(&mut self, id: ExecutionId) -> Result<Option<Completion>> {
        let Some(run) = self.runs.get_mut(&id) else {
            return Err(Error::ExecutionUnavailable);
        };
        if run
            .child
            .try_wait()
            .map_err(|_| Error::ExecutionUnavailable)?
            .is_none()
        {
            return Ok(None);
        }
        self.runs.remove(&id);
        Ok(Some(self.completion(id).unwrap_or(Completion {
            outcome: Outcome::MonitorLost,
            finished_at_ms: timestamp_ms(),
        })))
    }
    fn recover(&mut self, e: &Execution) -> Result<Option<Completion>> {
        let dir = self.root.run(e.id)?;
        let Some(_lock) = private_storage::try_lock(&dir.join("monitor.lock"))? else {
            return Ok(None);
        };
        Ok(Some(self.completion(e.id).unwrap_or(Completion {
            outcome: if e.supervisor_pid.is_none()
                && match std::fs::symlink_metadata(dir.join("launch-attempt.json")) {
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => true,
                    Ok(_) => false,
                    Err(_) => return Err(Error::StorageUnavailable),
                } {
                Outcome::Interrupted
            } else {
                Outcome::MonitorLost
            },
            finished_at_ms: timestamp_ms(),
        })))
    }
    fn logs(&self, id: ExecutionId) -> Result<(String, String)> {
        let dir = self.root.run(id)?;
        let read = |name: &str| -> Result<String> {
            let path = dir.join(name);
            if !path.exists() {
                return Ok(String::new());
            }
            let mut file = private_storage::private_file(&path)?;
            let size = file
                .metadata()
                .map_err(|_| Error::StorageUnavailable)?
                .len();
            use std::io::{Seek, SeekFrom};
            file.seek(SeekFrom::Start(size.saturating_sub(65536)))
                .map_err(|_| Error::StorageUnavailable)?;
            let mut bytes = vec![];
            file.take(65536)
                .read_to_end(&mut bytes)
                .map_err(|_| Error::StorageUnavailable)?;
            Ok(String::from_utf8_lossy(&bytes).into_owned())
        };
        Ok((read("stdout.log")?, read("stderr.log")?))
    }
    fn release(&mut self) {
        for run in self.runs.values_mut() {
            run.lease.take();
        }
    }
}
impl Drop for ProcessHost {
    fn drop(&mut self) {
        for run in self.runs.values_mut() {
            run.lease.take();
        }
    }
}
