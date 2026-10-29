//! `launchctl` calls for one user's graphical and background domains.
use super::print;
use crate::{
    bounded_command::{self, Failure, Output},
    jobs::Observation,
    operation_error::{Error, Result},
};
use std::{
    ffi::OsStr,
    path::{Path, PathBuf},
    process::Command,
    time::Duration,
};

const TIMEOUT: Duration = Duration::from_secs(5);
/// launchd spaces starts of the same job at least ten seconds apart, and
/// `kickstart` waits out the remainder before returning.
const KICKSTART_TIMEOUT: Duration = Duration::from_secs(20);
const MAX_OUTPUT: u64 = 1024 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Domain {
    /// A logged-in graphical session.
    Gui,
    /// The background session, available over ssh without a graphical login.
    User,
}
impl Domain {
    pub fn session_type(self) -> &'static str {
        match self {
            Self::Gui => "Aqua",
            Self::User => "Background",
        }
    }
}

pub struct Loaded {
    pub domain: Domain,
    pub observation: Observation,
    pub path: Option<PathBuf>,
}

pub struct Launchctl {
    program: PathBuf,
    user: u32,
}
impl Launchctl {
    pub fn new(program: PathBuf, user: u32) -> Self {
        Self { program, user }
    }
    fn domain(&self, domain: Domain) -> String {
        match domain {
            Domain::Gui => format!("gui/{}", self.user),
            Domain::User => format!("user/{}", self.user),
        }
    }
    fn target(&self, domain: Domain, label: &str) -> String {
        format!("{}/{label}", self.domain(domain))
    }
    fn run(&self, args: &[&OsStr]) -> Result<Output> {
        self.run_within(args, TIMEOUT)
    }
    fn run_within(&self, args: &[&OsStr], timeout: Duration) -> Result<Output> {
        let mut command = Command::new(&self.program);
        command.args(args);
        match bounded_command::run(command, timeout, MAX_OUTPUT) {
            Ok(output) => Ok(output),
            Err(Failure::TooLarge) => Err(Error::UnsupportedDefinition),
            Err(_) => Err(Error::SupervisorUnavailable),
        }
    }
    fn succeeds(&self, args: &[&OsStr]) -> Result<bool> {
        self.run(args).map(|o| o.status.success())
    }

    /// Where the job is loaded, from which file, and what launchd last observed.
    pub fn print(&self, label: &str) -> Result<Option<Loaded>> {
        for domain in [Domain::Gui, Domain::User] {
            let output = self.run(&["print".as_ref(), self.target(domain, label).as_ref()])?;
            if output.status.success() {
                let text = String::from_utf8_lossy(&output.stdout);
                return Ok(Some(Loaded {
                    domain,
                    observation: print::parse(&text),
                    path: print::path(&text),
                }));
            }
        }
        Ok(None)
    }
    /// The graphical session when someone is logged in, else the background one.
    pub fn session(&self) -> Result<Domain> {
        if self.succeeds(&["print".as_ref(), self.domain(Domain::Gui).as_ref()])? {
            Ok(Domain::Gui)
        } else if self.succeeds(&["print".as_ref(), self.domain(Domain::User).as_ref()])? {
            Ok(Domain::User)
        } else {
            Err(Error::SupervisorUnavailable)
        }
    }
    pub fn bootstrap(&self, domain: Domain, path: &Path) -> Result<()> {
        // launchd finishes a preceding bootout asynchronously, so a reload can
        // briefly fail with an I/O error or "in progress" while the old
        // instance is still being removed. Anything else is a refusal.
        const BUSY: [i32; 2] = [5, 37];
        for attempt in 0..20 {
            let output = self.run(&[
                "bootstrap".as_ref(),
                self.domain(domain).as_ref(),
                path.as_os_str(),
            ])?;
            match output.status.code() {
                Some(0) => return Ok(()),
                Some(code) if BUSY.contains(&code) && attempt < 19 => {
                    std::thread::sleep(Duration::from_millis(100))
                }
                _ => break,
            }
        }
        Err(Error::Refused)
    }
    pub fn bootout(&self, domain: Domain, label: &str) -> Result<()> {
        if self.succeeds(&["bootout".as_ref(), self.target(domain, label).as_ref()])? {
            Ok(())
        } else {
            Err(Error::SupervisorUnavailable)
        }
    }
    pub fn kickstart(&self, domain: Domain, label: &str) -> Result<()> {
        let target = self.target(domain, label);
        let output =
            self.run_within(&["kickstart".as_ref(), target.as_ref()], KICKSTART_TIMEOUT)?;
        if output.status.success() {
            Ok(())
        } else {
            Err(Error::SupervisorUnavailable)
        }
    }
}
