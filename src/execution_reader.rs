//! Lookup boundary for global execution IDs used by runtime log queries.
use crate::{
    execution_records::*,
    operation_error::{Error, Result},
    private_storage::Root,
};
pub(crate) struct SqliteExecutionReader(rusqlite::Connection);
impl SqliteExecutionReader {
    pub fn open(root: &Root) -> Result<Self> {
        Ok(Self(crate::database::open(root)?))
    }
    pub fn get(&self, id: ExecutionId) -> Result<Execution> {
        crate::execution_storage::read(&self.0, "SELECT * FROM executions WHERE id=?1", id.into())?
            .into_iter()
            .next()
            .ok_or(Error::NotFound)
    }
}
