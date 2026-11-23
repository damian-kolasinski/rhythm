//! Showing a file where the operator can act on it: selected in a Finder
//! window.
use std::{
    cell::RefCell,
    path::{Path, PathBuf},
    process::{Command, Stdio},
    rc::Rc,
};

pub trait Finder {
    /// Opens a Finder window with `path` selected. A file that has gone opens
    /// nothing; the window shows its path anyway.
    fn reveal(&self, path: &Path);
}

/// The Finder of this Mac.
pub struct System;

impl Finder for System {
    fn reveal(&self, path: &Path) {
        let child = Command::new("/usr/bin/open")
            .arg("-R")
            .arg(path)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn();
        // Reap `open` off the window's thread; it returns once Finder has the request.
        if let Ok(mut child) = child {
            std::thread::spawn(move || child.wait());
        }
    }
}

/// Remembers what scenes asked to reveal instead of opening Finder.
#[derive(Clone, Default)]
pub struct Sample {
    pub revealed: Rc<RefCell<Vec<PathBuf>>>,
}

impl Finder for Sample {
    fn reveal(&self, path: &Path) {
        self.revealed.borrow_mut().push(path.to_owned());
    }
}
