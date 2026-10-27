use super::*;

pub trait RuntimeSupervisor {
    fn install(&self, executable: &std::path::Path, directory: &std::path::Path) -> Result<()>;
    fn uninstall(&self, directory: &std::path::Path) -> Result<bool>;
}

pub trait ServiceStore {
    fn create(&mut self, definition: &Definition) -> Result<()>;
    fn list(&self) -> Result<Vec<StoredService>>;
    fn get(&self, id: &ServiceId) -> Result<StoredService>;
    fn desire(&mut self, id: &ServiceId, desired: Desired) -> Result<()>;
    fn request_start(&mut self, id: &ServiceId) -> Result<()>;
    fn begin(&mut self, id: &ServiceId, now: i64) -> Result<ExecutionId>;
    fn started(&mut self, id: ExecutionId, pid: u32) -> Result<()>;
    fn finish(&mut self, id: ExecutionId, completion: &Completion) -> Result<()>;
    /// Record operator confirmation that uncertain execution has been cleaned up; stop desired work.
    fn acknowledge(&mut self, id: ExecutionId, now: i64) -> Result<()>;
    fn history(&self, id: &ServiceId) -> Result<Vec<Execution>>;
    fn active(&self) -> Result<Vec<Execution>>;
}
