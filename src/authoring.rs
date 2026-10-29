//! Create, adopt, release, and remove jobs so that every one records its runs
//! and starts from the same predictable environment.
use crate::{
    jobs::{JobSpec, Label, Observation, Schedule, Supervisor},
    operation_error::{Error, Result},
    wrapper_command::Wrapped,
};
use std::{
    collections::BTreeMap,
    os::unix::ffi::OsStrExt,
    path::{Path, PathBuf},
};

/// Labels of jobs created by `rhythm add`.
pub const OWNED_PREFIX: &str = "dev.rhythm.";

/// How job definitions reach the recorder.
pub struct Recorder {
    pub executable: PathBuf,
    pub data_dir: Option<PathBuf>,
    /// Where the recorder's own messages go when it cannot record.
    pub diagnostics: PathBuf,
}
impl Recorder {
    fn wrap(&self, label: &Label, forward: bool, program: Vec<String>) -> Result<Vec<String>> {
        if !self.executable.is_absolute() {
            return Err(Error::InvalidRequest);
        }
        Ok(Wrapped {
            executable: self.executable.clone(),
            data_dir: self.data_dir.clone(),
            label: label.clone(),
            forward,
            program,
        }
        .arguments())
    }
}

/// A short name such as `report` means `dev.rhythm.report`; anything with a dot
/// is a full launchd label.
pub fn resolve(reference: &str) -> Result<Label> {
    if reference.contains('.') {
        reference.parse()
    } else {
        name(reference)
    }
}

fn name(name: &str) -> Result<Label> {
    let valid = (1..=63).contains(&name.len())
        && name.starts_with(|c: char| c.is_ascii_lowercase() || c.is_ascii_digit())
        && name
            .bytes()
            .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if !valid {
        return Err(Error::InvalidRequest);
    }
    format!("{OWNED_PREFIX}{name}").parse()
}

pub struct NewJob {
    pub name: String,
    pub schedule: Schedule,
    pub command: Vec<String>,
    /// Defaults to the home directory, which outlives agent worktrees.
    pub working_directory: Option<PathBuf>,
    pub environment: Vec<String>,
    /// Replace an existing job Rhythm created under the same name.
    pub replace: bool,
}

/// Context the caller provides from its own environment.
pub struct Caller<'a> {
    pub home: &'a Path,
    pub current_directory: &'a Path,
    /// The caller's PATH, used only to resolve a bare command name.
    pub search_path: Option<&'a str>,
}

#[derive(Debug, PartialEq, Eq)]
pub enum Added {
    Created,
    Replaced,
}

pub fn add(
    supervisor: &impl Supervisor,
    recorder: &Recorder,
    caller: &Caller,
    job: NewJob,
) -> Result<(Label, Added)> {
    let label = name(&job.name)?;
    let (program, arguments) = job.command.split_first().ok_or(Error::InvalidRequest)?;
    let executable = executable(program, caller)?;
    let working_directory = match job.working_directory {
        Some(dir) => absolute(&dir, caller)?
            .canonicalize()
            .map_err(|_| Error::InvalidRequest)?,
        None => caller.home.to_owned(),
    };
    if !working_directory.is_dir() {
        return Err(Error::InvalidRequest);
    }
    let mut environment = BTreeMap::from([("PATH".to_owned(), default_path(caller.home)?)]);
    for entry in &job.environment {
        let (key, value) = entry.split_once('=').ok_or(Error::InvalidRequest)?;
        let valid = key.starts_with(|c: char| c.is_ascii_alphabetic() || c == '_')
            && key.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
            && !value.contains('\0');
        if !valid {
            return Err(Error::InvalidRequest);
        }
        environment.insert(key.to_owned(), value.to_owned());
    }
    let program: Vec<String> = std::iter::once(executable)
        .chain(arguments.iter().cloned())
        .collect();
    let spec = JobSpec {
        arguments: recorder.wrap(&label, false, program)?,
        label: label.clone(),
        schedule: job.schedule,
        working_directory,
        environment,
        diagnostics: recorder.diagnostics.clone(),
    };
    if !job.replace {
        supervisor.create(&spec)?;
        return Ok((label, Added::Created));
    }
    match owned(supervisor, &label) {
        Ok(true) => supervisor.replace(&spec).map(|_| (label, Added::Replaced)),
        Ok(false) => Err(Error::NotOwned),
        Err(Error::JobNotFound) => supervisor.create(&spec).map(|_| (label, Added::Created)),
        Err(error) => Err(error),
    }
}

/// Rhythm owns exactly the jobs `add` created: its prefix, run through the
/// recorder under their own label.
fn owned(supervisor: &impl Supervisor, label: &Label) -> Result<bool> {
    Ok(label.as_str().starts_with(OWNED_PREFIX)
        && Wrapped::parse(&supervisor.program(label)?.arguments).is_some_and(|w| w.label == *label))
}

/// Mise shims and user tools first, because launchd's own PATH has neither.
fn default_path(home: &Path) -> Result<String> {
    let home = home.to_str().ok_or(Error::InvalidRequest)?;
    Ok(format!(
        "{home}/.local/bin:{home}/.local/share/mise/shims:/opt/homebrew/bin:/usr/local/bin:/usr/bin:/bin:/usr/sbin:/sbin"
    ))
}

/// Absolute without resolving symlinks: a mise shim must stay a shim.
fn absolute(path: &Path, caller: &Caller) -> Result<PathBuf> {
    std::path::absolute(caller.current_directory.join(path)).map_err(|_| Error::InvalidRequest)
}

/// An absolute path to an existing executable file.
fn executable(program: &str, caller: &Caller) -> Result<String> {
    let path = if program.contains('/') {
        absolute(Path::new(program), caller)?
    } else {
        caller
            .search_path
            .unwrap_or_default()
            .split(':')
            .filter(|dir| dir.starts_with('/'))
            .map(|dir| Path::new(dir).join(program))
            .find(|candidate| is_executable(candidate))
            .ok_or(Error::InvalidRequest)?
    };
    if !is_executable(&path) {
        return Err(Error::InvalidRequest);
    }
    path.into_os_string()
        .into_string()
        .map_err(|_| Error::InvalidRequest)
}

/// A regular file this user may execute.
fn is_executable(path: &Path) -> bool {
    let Ok(c_path) = std::ffi::CString::new(path.as_os_str().as_bytes()) else {
        return false;
    };
    std::fs::metadata(path).is_ok_and(|m| m.is_file())
        && unsafe { libc::access(c_path.as_ptr(), libc::X_OK) } == 0
}

#[derive(Debug, PartialEq, Eq)]
pub enum Adoption {
    Adopted,
    /// Already recorded, through another Rhythm executable or state directory.
    Repointed,
    AlreadyRecorded,
}

/// Starts recording an existing job's runs without changing anything else.
/// Reloading stops a running instance, so that needs `restart`.
pub fn adopt(
    supervisor: &impl Supervisor,
    recorder: &Recorder,
    label: &Label,
    restart: bool,
) -> Result<Adoption> {
    let program = supervisor.program(label)?;
    let (forward, original, adoption) = match Wrapped::parse(&program.arguments) {
        Some(wrapped) if wrapped.label != *label => return Err(Error::UnsupportedDefinition),
        Some(wrapped)
            if wrapped.executable == recorder.executable
                && wrapped.data_dir == recorder.data_dir =>
        {
            return Ok(Adoption::AlreadyRecorded);
        }
        Some(wrapped) => (wrapped.forward, wrapped.program, Adoption::Repointed),
        None => (program.has_log_files, program.arguments, Adoption::Adopted),
    };
    refuse_running(supervisor, label, restart)?;
    supervisor.set_program(label, &recorder.wrap(label, forward, original)?)?;
    Ok(adoption)
}

/// Restores the job's original command; its recorded history stays.
pub fn release(supervisor: &impl Supervisor, label: &Label, restart: bool) -> Result<()> {
    let program = supervisor.program(label)?;
    let wrapped = Wrapped::parse(&program.arguments)
        .filter(|w| w.label == *label)
        .ok_or(Error::NotRecorded)?;
    refuse_running(supervisor, label, restart)?;
    supervisor.set_program(label, &wrapped.program)
}

/// Unloads and deletes a job created by `add`; its recorded history stays.
pub fn remove(supervisor: &impl Supervisor, label: &Label) -> Result<()> {
    if !owned(supervisor, label)? {
        return Err(Error::NotOwned);
    }
    supervisor.uninstall(label)
}

fn refuse_running(supervisor: &impl Supervisor, label: &Label, restart: bool) -> Result<()> {
    match supervisor.observe(label)? {
        Observation::Loaded { pid: Some(_), .. } if !restart => Err(Error::RestartRequired),
        _ => Ok(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_names_become_rhythm_labels() {
        assert_eq!(resolve("report").unwrap().as_str(), "dev.rhythm.report");
        assert_eq!(
            resolve("com.example.scan").unwrap().as_str(),
            "com.example.scan"
        );
        for bad in ["", "Report", "-x", "a_b", "a b", &"x".repeat(64)] {
            assert!(resolve(bad).is_err(), "{bad:?}");
        }
    }
}
