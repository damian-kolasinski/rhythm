use super::*;
use crate::{execution_records::*, operation_error::Result};
pub trait Clock {
    fn now_ms(&self) -> i64;
}
pub struct SystemClock;
impl Clock for SystemClock {
    fn now_ms(&self) -> i64 {
        timestamp_ms()
    }
}
pub trait ScheduleStore {
    fn create(&mut self, schedule: &Stored) -> Result<()>;
    fn list(&self) -> Result<Vec<Stored>>;
    fn get(&self, id: &ScheduleId) -> Result<Stored>;
    fn save(&mut self, schedule: &Stored) -> Result<()>;
    /// Consume the pending invocation and allocate its execution atomically.
    fn begin(&mut self, id: &ScheduleId, now: i64) -> Result<ExecutionId>;
    fn started(&mut self, id: ExecutionId, pid: u32) -> Result<()>;
    fn finish(&mut self, id: ExecutionId, completion: &Completion) -> Result<()>;
    fn acknowledge(&mut self, id: ExecutionId, now: i64) -> Result<()>;
    fn history(&self, id: &ScheduleId) -> Result<Vec<Execution>>;
    fn active(&self) -> Result<Vec<Execution>>;
}
