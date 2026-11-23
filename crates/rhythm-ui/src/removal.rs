//! Removing a routine from the window: reading what removal leaves behind,
//! the operator's confirmation, the removal itself, and what is left to say
//! once it is gone.
use std::path::PathBuf;

use rhythm::{
    authoring::Removal,
    jobs::{Health, Label},
    operation_error::{Error, Result},
};

use crate::routine::Routine;

/// The routine the confirmation dialog is about.
pub struct Flow {
    /// The routine's row.
    pub key: String,
    pub label: Label,
    pub name: String,
    /// The label and trigger, as the row shows them.
    pub about: String,
    /// Removing stops the run in progress.
    pub running: bool,
    pub stage: Stage,
}

pub enum Stage {
    /// Reading the routine's definition for the script it runs.
    Reading,
    /// What removal leaves behind is known; waiting for the operator.
    Ready(Removal),
    /// The definition could not be read, so the window cannot say what
    /// removal would leave behind and does not offer it.
    Unreadable(Error),
    Removing(Removal),
    /// The removal failed; the routine is still there to try again.
    Failed(Removal, Error),
}

/// What the machine answered about the routine `label`.
pub enum Reply {
    Read(Label, Result<Removal>),
    Removed(Label, Result<Removal>),
}

/// What the window says once a routine is gone.
#[derive(Debug, PartialEq, Eq)]
pub struct Removed {
    pub name: String,
    pub script: Option<PathBuf>,
}

/// What the window does after the flow took a reply.
pub enum Next {
    /// Ask the machine to remove the routine.
    Remove(Label),
    /// The routine is gone: hide its row.
    Gone(String, Removed),
    Nothing,
}

impl Flow {
    /// The flow for a routine the window can remove; read its definition next.
    pub fn open(routine: &Routine) -> Option<Self> {
        Some(Self {
            key: routine.key.clone(),
            label: routine.removable.clone()?,
            name: routine.name.clone(),
            about: format!("{} · {}", routine.label, routine.trigger),
            running: routine.state == Health::Running,
            stage: Stage::Reading,
        })
    }

    /// The operator confirmed; only a known removal can start.
    pub fn confirm(&mut self) -> Next {
        let removal = match &self.stage {
            Stage::Ready(removal) | Stage::Failed(removal, _) => removal.clone(),
            Stage::Reading | Stage::Unreadable(_) | Stage::Removing(_) => return Next::Nothing,
        };
        self.stage = Stage::Removing(removal);
        Next::Remove(self.label.clone())
    }

    /// Whether the dialog may close: not while the removal runs, so its
    /// outcome is always shown.
    pub fn closable(&self) -> bool {
        !matches!(self.stage, Stage::Removing(_))
    }

    /// Takes a reply about this flow's routine; replies about another
    /// routine, or to a question no longer asked, change nothing.
    pub fn receive(&mut self, reply: Reply) -> Next {
        match (reply, &self.stage) {
            (Reply::Read(label, result), Stage::Reading) if label == self.label => {
                self.stage = match result {
                    Ok(removal) => Stage::Ready(removal),
                    Err(error) => Stage::Unreadable(error),
                };
                Next::Nothing
            }
            (Reply::Removed(label, result), Stage::Removing(removal)) if label == self.label => {
                match result {
                    Ok(left) => Next::Gone(
                        self.key.clone(),
                        Removed {
                            name: self.name.clone(),
                            script: left.script,
                        },
                    ),
                    Err(error) => {
                        self.stage = Stage::Failed(removal.clone(), error);
                        Next::Nothing
                    }
                }
            }
            _ => Next::Nothing,
        }
    }

    /// The script removal leaves behind, once known.
    pub fn script(&self) -> Option<&PathBuf> {
        match &self.stage {
            Stage::Ready(removal) | Stage::Removing(removal) | Stage::Failed(removal, _) => {
                removal.script.as_ref()
            }
            Stage::Reading | Stage::Unreadable(_) => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn flow(stage: Stage) -> Flow {
        Flow {
            key: "UserAgent/0/dev.rhythm.example".into(),
            label: "dev.rhythm.example".parse().unwrap(),
            name: "Example".into(),
            about: "dev.rhythm.example · Every 15m".into(),
            running: false,
            stage,
        }
    }

    fn label(text: &str) -> Label {
        text.parse().unwrap()
    }

    fn nothing_left() -> Result<Removal> {
        Ok(Removal { script: None })
    }

    #[test]
    fn replies_about_another_routine_or_an_earlier_question_change_nothing() {
        let mut reading = flow(Stage::Reading);
        let other = Reply::Read(label("dev.rhythm.other"), nothing_left());
        assert!(matches!(reading.receive(other), Next::Nothing));
        assert!(matches!(reading.stage, Stage::Reading));
        let early = Reply::Removed(label("dev.rhythm.example"), nothing_left());
        assert!(matches!(reading.receive(early), Next::Nothing));
        assert!(
            matches!(reading.confirm(), Next::Nothing),
            "nothing known yet"
        );

        let mut removing = flow(Stage::Removing(Removal { script: None }));
        let late = Reply::Read(label("dev.rhythm.example"), Err(Error::JobNotFound));
        assert!(matches!(removing.receive(late), Next::Nothing));
        assert!(matches!(removing.stage, Stage::Removing(_)));
        assert!(!removing.closable());
        let done = Reply::Removed(label("dev.rhythm.example"), nothing_left());
        assert!(matches!(
            removing.receive(done),
            Next::Gone(key, Removed { script: None, .. }) if key == "UserAgent/0/dev.rhythm.example"
        ));
    }
}
