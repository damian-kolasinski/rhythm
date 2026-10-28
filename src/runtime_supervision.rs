//! Platform boundary for keeping the scheduler runtime available.
use crate::operation_error::Result;
use std::path::Path;

pub trait RuntimeSupervisor {
    fn install(&self, executable: &Path, directory: &Path) -> Result<()>;
    fn uninstall(&self, directory: &Path) -> Result<bool>;
}
