//! What the window changes about routines on this Mac: removing one Rhythm
//! created or adopted, after reading what that leaves behind.
use std::{collections::BTreeMap, path::PathBuf, sync::Mutex};

use rhythm::{
    authoring::{self, Removal},
    jobs::Label,
    operation_error::{Error, Result},
};

pub trait RoutineControl: Send + Sync {
    /// What removing the routine would leave behind, without changing it.
    fn removal(&self, label: &Label) -> Result<Removal>;
    /// Removes the routine, stopping a run in progress.
    fn remove(&self, label: &Label) -> Result<Removal>;
}

/// The user's launchd jobs.
pub struct Launchd;

impl RoutineControl for Launchd {
    fn removal(&self, label: &Label) -> Result<Removal> {
        authoring::removal(&rhythm::launchd::Launchd::current_user()?, label)
    }

    fn remove(&self, label: &Label) -> Result<Removal> {
        authoring::remove(&rhythm::launchd::Launchd::current_user()?, label)
    }
}

/// Synthetic routines for scenes and the demonstration: the label of each
/// removable one and the script it runs, if any.
#[derive(Default)]
pub struct Sample {
    routines: Mutex<BTreeMap<Label, Result<Removal>>>,
}

impl Sample {
    pub fn new(routines: impl IntoIterator<Item = (&'static str, Result<Removal>)>) -> Self {
        Self {
            routines: Mutex::new(
                routines
                    .into_iter()
                    .map(|(label, removal)| (label.parse().expect("a sample label"), removal))
                    .collect(),
            ),
        }
    }

    pub fn script(path: &str) -> Result<Removal> {
        Ok(Removal {
            script: Some(PathBuf::from(path)),
        })
    }
}

impl RoutineControl for Sample {
    fn removal(&self, label: &Label) -> Result<Removal> {
        let routines = self.routines.lock().unwrap();
        routines
            .get(label)
            .cloned()
            .unwrap_or(Err(Error::JobNotFound))
    }

    fn remove(&self, label: &Label) -> Result<Removal> {
        let mut routines = self.routines.lock().unwrap();
        routines.remove(label).unwrap_or(Err(Error::JobNotFound))
    }
}
