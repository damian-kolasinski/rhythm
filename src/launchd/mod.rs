//! User launchd adapter: job definitions in the LaunchAgents directory, observed
//! through `launchctl` in the graphical or background user domain.
mod definition;
mod print;

use crate::{
    bounded_command::{self, Failure},
    jobs::{Activation, Job, Observation, Supervisor},
    operation_error::{Error, Result},
};
use definition::Parsed;
use std::{
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const MAX_DEFINITION: u64 = 1024 * 1024;
const LAUNCHCTL_TIMEOUT: Duration = Duration::from_secs(5);
/// Concurrent `launchctl` processes while observing jobs.
const PARALLEL: usize = 8;

pub struct Launchd {
    agents: PathBuf,
    launchctl: PathBuf,
    user: u32,
}
impl Launchd {
    pub fn current_user() -> Result<Self> {
        if !cfg!(target_os = "macos") {
            return Err(Error::UnsupportedPlatform);
        }
        let home = std::env::var_os("HOME")
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .ok_or(Error::InvalidRequest)?;
        Ok(Self::at_boundary(
            home.join("Library/LaunchAgents"),
            "/bin/launchctl".into(),
            unsafe { libc::getuid() },
        ))
    }
    pub fn at_boundary(agents: PathBuf, launchctl: PathBuf, user: u32) -> Self {
        Self {
            agents,
            launchctl,
            user,
        }
    }
    fn domains(&self) -> [String; 2] {
        [format!("gui/{}", self.user), format!("user/{}", self.user)]
    }
    fn observe(&self, label: &str) -> Result<Observation> {
        for domain in self.domains() {
            let mut command = Command::new(&self.launchctl);
            command.arg("print").arg(format!("{domain}/{label}"));
            let output = match bounded_command::run(command, LAUNCHCTL_TIMEOUT, MAX_DEFINITION) {
                Ok(output) => output,
                Err(Failure::Unavailable | Failure::TimedOut) => {
                    return Err(Error::SupervisorUnavailable);
                }
                Err(Failure::TooLarge) => continue,
            };
            if output.status.success() {
                return Ok(print::parse(&String::from_utf8_lossy(&output.stdout)));
            }
        }
        Ok(Observation::Unloaded)
    }
}
impl Supervisor for Launchd {
    fn jobs(&self) -> Result<Vec<Job>> {
        let entries = match fs::read_dir(&self.agents) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
            Err(_) => return Err(Error::DefinitionsUnavailable),
        };
        let mut paths: Vec<PathBuf> = entries
            .filter_map(|e| e.ok().map(|e| e.path()))
            .filter(|p| {
                p.extension().is_some_and(|e| e == "plist")
                    && p.file_name()
                        .is_some_and(|n| !n.as_encoded_bytes().starts_with(b"."))
            })
            .collect();
        paths.sort();
        let definitions: Vec<(String, Option<Activation>)> = paths
            .iter()
            .filter_map(|path| {
                match read(path).map_or(Parsed::Invalid, |b| definition::parse(&b)) {
                    Parsed::Job(d) => Some((d.label, Some(d.activation))),
                    Parsed::Empty => None,
                    Parsed::Invalid => Some((
                        path.file_stem().unwrap().to_string_lossy().into_owned(),
                        None,
                    )),
                }
            })
            .collect();
        let mut jobs = Vec::with_capacity(definitions.len());
        for batch in definitions.chunks(PARALLEL) {
            let observations: Vec<Result<Observation>> = std::thread::scope(|scope| {
                let pending: Vec<_> = batch
                    .iter()
                    .map(|(label, _)| scope.spawn(|| self.observe(label)))
                    .collect();
                pending
                    .into_iter()
                    .map(|p| p.join().unwrap_or(Err(Error::SupervisorUnavailable)))
                    .collect()
            });
            for ((label, activation), observation) in batch.iter().cloned().zip(observations) {
                jobs.push(Job {
                    label,
                    activation,
                    observation: observation?,
                });
            }
        }
        Ok(jobs)
    }
}

/// Only regular files: a FIFO or device named `*.plist` must not block status.
fn read(path: &Path) -> Option<Vec<u8>> {
    if !fs::metadata(path).ok()?.is_file() {
        return None;
    }
    let file = fs::File::options()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .ok()?;
    if !file.metadata().ok()?.is_file() {
        return None;
    }
    let mut bytes = vec![];
    file.take(MAX_DEFINITION + 1).read_to_end(&mut bytes).ok()?;
    (bytes.len() as u64 <= MAX_DEFINITION).then_some(bytes)
}
