//! launchd adapter: files describe registrations; launchctl describes observation.
mod observation;
mod registration;

use crate::discovery::{Diagnostic, DiagnosticCode, InventorySource, ObservedState, Registration};
use std::{fs, path::PathBuf, process::Command};

pub struct LaunchdInventory {
    directory: PathBuf,
    launchctl: PathBuf,
}

impl LaunchdInventory {
    pub fn new(directory: PathBuf) -> Self {
        Self {
            directory,
            launchctl: "/bin/launchctl".into(),
        }
    }

    /// Allows a platform command at the system boundary to be replaced in tests.
    pub fn with_launchctl(mut self, executable: PathBuf) -> Self {
        self.launchctl = executable;
        self
    }
}

impl InventorySource for LaunchdInventory {
    fn discover(&self) -> (Vec<Registration>, Vec<Diagnostic>) {
        let mut diagnostics = Vec::new();
        let observation = Command::new(&self.launchctl)
            .arg("list")
            .output()
            .ok()
            .filter(|o| o.status.success())
            .and_then(|o| String::from_utf8(o.stdout).ok())
            .and_then(|s| observation::parse(&s));
        if observation.is_none() {
            diagnostics.push(Diagnostic {
                source: None,
                code: DiagnosticCode::ObservationUnavailable,
            });
        }
        let entries = match fs::read_dir(&self.directory) {
            Ok(entries) => entries,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return (vec![], diagnostics),
            Err(_) => {
                diagnostics.push(Diagnostic {
                    source: Some(self.directory.clone()),
                    code: DiagnosticCode::DirectoryUnreadable,
                });
                return (vec![], diagnostics);
            }
        };
        let mut registrations = Vec::new();
        for entry in entries {
            let path = match entry {
                Ok(entry) => entry.path(),
                Err(_) => {
                    diagnostics.push(Diagnostic {
                        source: Some(self.directory.clone()),
                        code: DiagnosticCode::EntryUnreadable,
                    });
                    continue;
                }
            };
            if path.extension().is_none_or(|ext| ext != "plist") {
                continue;
            }
            // Parse errors contain source snippets; never return their messages.
            match registration::read(&path) {
                Ok(mut registration) => {
                    registration.observed = match &observation {
                        Some(states) => states
                            .get(&registration.label)
                            .cloned()
                            .unwrap_or(ObservedState::NotObserved),
                        None => ObservedState::Unknown,
                    };
                    registrations.push(registration);
                }
                Err(code) => diagnostics.push(Diagnostic {
                    source: Some(path),
                    code,
                }),
            }
        }
        diagnostics.sort_by(|a, b| a.source.cmp(&b.source));
        (registrations, diagnostics)
    }
}
