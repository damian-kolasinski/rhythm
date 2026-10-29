//! User launchd adapter: job definitions in the LaunchAgents directory, observed
//! and controlled through `launchctl` in the graphical or background user domain.
mod definition;
mod launchctl;
mod print;
mod registration;
mod render;

use crate::{
    jobs::{Job, JobSpec, Label, Observation, Program, Supervisor},
    operation_error::{Error, Result},
};
use definition::Parsed;
use launchctl::{Launchctl, Loaded};
use registration::Change;
use std::{
    fs,
    io::Read,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
};

const MAX_DEFINITION: u64 = 1024 * 1024;
/// Concurrent `launchctl` processes while observing jobs.
const PARALLEL: usize = 8;

pub struct Launchd {
    agents: PathBuf,
    launchctl: Launchctl,
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
            launchctl: Launchctl::new(launchctl, user),
        }
    }
    fn observe_label(&self, label: &str) -> Result<Observation> {
        Ok(self
            .launchctl
            .print(label)?
            .map_or(Observation::Unloaded, |l| l.observation))
    }
    fn definitions(&self) -> Result<Vec<PathBuf>> {
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
        Ok(paths)
    }
    /// The definition file declaring `label`: the file launchd loaded it from
    /// when loaded, else the conventional name, else any other.
    fn locate(
        &self,
        label: &Label,
        loaded: Option<&Loaded>,
    ) -> Result<(PathBuf, plist::Dictionary, Vec<u8>)> {
        if let Some(path) = loaded.and_then(|l| l.path.as_ref()) {
            let bytes = read(path).ok_or(Error::UnsupportedDefinition)?;
            let plist = definition::dictionary(&bytes).ok_or(Error::UnsupportedDefinition)?;
            return Ok((path.clone(), plist, bytes));
        }
        let conventional = self.conventional(label);
        let candidates = std::iter::once(conventional.clone()).chain(
            self.definitions()?
                .into_iter()
                .filter(|p| *p != conventional),
        );
        for path in candidates {
            if let Some(bytes) = read(&path)
                && let Some(plist) = definition::dictionary(&bytes)
                && plist.get("Label").and_then(plist::Value::as_string) == Some(label.as_str())
            {
                return Ok((path, plist, bytes));
            }
        }
        Err(Error::JobNotFound)
    }
    fn conventional(&self, label: &Label) -> PathBuf {
        self.agents.join(format!("{label}.plist"))
    }
    /// Rhythm rewrites only regular files in LaunchAgents it can replace atomically.
    fn writable(&self, path: &Path) -> Result<()> {
        // launchd reports resolved paths, such as /private/var for /var.
        let parent = path.parent().and_then(|p| p.canonicalize().ok());
        if parent.is_none() || parent != self.agents.canonicalize().ok() {
            return Err(Error::UnsupportedDefinition);
        }
        match fs::symlink_metadata(path) {
            Ok(m) if m.file_type().is_file() => Ok(()),
            Ok(_) => Err(Error::UnsupportedDefinition),
            Err(_) => Err(Error::DefinitionsUnavailable),
        }
    }
}
impl Supervisor for Launchd {
    fn jobs(&self) -> Result<Vec<Job>> {
        let mut jobs: Vec<Job> = self
            .definitions()?
            .iter()
            .filter_map(|path| {
                let (label, activation, recorded) =
                    match read(path).map_or(Parsed::Invalid, |b| definition::parse(&b)) {
                        Parsed::Job(d) => (d.label, Some(d.activation), d.recorded),
                        Parsed::Empty => return None,
                        Parsed::Invalid => (
                            path.file_stem().unwrap().to_string_lossy().into_owned(),
                            None,
                            None,
                        ),
                    };
                Some(Job {
                    label,
                    activation,
                    recorded,
                    observation: Observation::Unloaded,
                })
            })
            .collect();
        for batch in jobs.chunks_mut(PARALLEL) {
            let observations: Vec<Result<Observation>> = std::thread::scope(|scope| {
                let pending: Vec<_> = batch
                    .iter()
                    .map(|job| scope.spawn(|| self.observe_label(&job.label)))
                    .collect();
                pending
                    .into_iter()
                    .map(|p| p.join().unwrap_or(Err(Error::SupervisorUnavailable)))
                    .collect()
            });
            for (job, observation) in batch.iter_mut().zip(observations) {
                job.observation = observation?;
            }
        }
        Ok(jobs)
    }
    fn observe(&self, label: &Label) -> Result<Observation> {
        self.observe_label(label.as_str())
    }
    fn program(&self, label: &Label) -> Result<Program> {
        let loaded = self.launchctl.print(label.as_str())?;
        let (_, plist, _) = self.locate(label, loaded.as_ref())?;
        definition::program(&plist).ok_or(Error::UnsupportedDefinition)
    }
    fn create(&self, spec: &JobSpec) -> Result<()> {
        if self.launchctl.print(spec.label.as_str())?.is_some() {
            return Err(Error::AlreadyExists);
        }
        match self.locate(&spec.label, None) {
            Err(Error::JobNotFound) => {}
            Ok(_) => return Err(Error::AlreadyExists),
            Err(error) => return Err(error),
        }
        let domain = self.launchctl.session()?;
        let new = render::job(spec, domain)?;
        // Creation is exclusive: any file already at the path, whatever it
        // declares, is left alone.
        registration::apply(
            &self.launchctl,
            Change {
                path: &self.conventional(&spec.label),
                label: spec.label.as_str(),
                new: &new,
                old: None,
                loaded: None,
                load: Some(domain),
            },
        )
    }
    fn replace(&self, spec: &JobSpec) -> Result<()> {
        let loaded = self.launchctl.print(spec.label.as_str())?;
        let (path, _, old) = self.locate(&spec.label, loaded.as_ref())?;
        self.writable(&path)?;
        let domain = match &loaded {
            Some(loaded) => loaded.domain,
            None => self.launchctl.session()?,
        };
        let new = render::job(spec, domain)?;
        registration::apply(
            &self.launchctl,
            Change {
                path: &path,
                label: spec.label.as_str(),
                new: &new,
                old: Some(&old),
                loaded: loaded.map(|l| l.domain),
                load: Some(domain),
            },
        )
    }
    fn set_program(&self, label: &Label, arguments: &[String]) -> Result<()> {
        let loaded = self.launchctl.print(label.as_str())?;
        let (path, plist, old) = self.locate(label, loaded.as_ref())?;
        self.writable(&path)?;
        definition::program(&plist).ok_or(Error::UnsupportedDefinition)?;
        let domain = loaded.map(|l| l.domain);
        let new = render::with_program(plist, arguments)?;
        registration::apply(
            &self.launchctl,
            Change {
                path: &path,
                label: label.as_str(),
                new: &new,
                old: Some(&old),
                loaded: domain,
                load: domain,
            },
        )
    }
    fn uninstall(&self, label: &Label) -> Result<()> {
        let loaded = self.launchctl.print(label.as_str())?;
        let (path, _, _) = self.locate(label, loaded.as_ref())?;
        self.writable(&path)?;
        if let Some(loaded) = loaded {
            self.launchctl.bootout(loaded.domain, label.as_str())?;
        }
        fs::remove_file(path).map_err(|_| Error::DefinitionsUnavailable)
    }
    fn start(&self, label: &Label) -> Result<()> {
        let Some(loaded) = self.launchctl.print(label.as_str())? else {
            return Err(Error::JobNotLoaded);
        };
        match self.launchctl.kickstart(loaded.domain, label.as_str()) {
            // launchd refuses to start a job that started on its own meanwhile.
            Err(error) => match self.launchctl.print(label.as_str())? {
                Some(Loaded {
                    observation: Observation::Loaded { pid: Some(_), .. },
                    ..
                }) => Err(Error::Busy),
                _ => Err(error),
            },
            ok => ok,
        }
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
