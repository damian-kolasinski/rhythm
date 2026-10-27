use crate::{execution_command::CommandSpec, execution_records::*, operation_error::Result};

/// A host owns process containment, cancellation and durable completion evidence.
pub trait ExecutionHost {
    fn start(&mut self, id: ExecutionId, definition: &CommandSpec) -> Result<u32>;
    fn stop(&mut self, id: ExecutionId) -> Result<()>;
    fn poll(&mut self, id: ExecutionId) -> Result<Option<Completion>>;
    fn recover(&mut self, execution: &Execution) -> Result<Recovery>;
    fn logs(&self, id: ExecutionId) -> Result<(String, String)>;
    fn release(&mut self);
}
