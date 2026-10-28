use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    InvalidRequest,
    NotFound,
    AlreadyExists,
    Busy,
    StorageUnavailable,
    UnsupportedSchema,
    ExecutionUnavailable,
    RecoveryRequired,
    RuntimeUnavailable,
    RuntimeAlreadyRunning,
    InsecureStorage,
    Unauthorized,
    UnsupportedPlatform,
}
impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}",
            serde_json::to_value(self).unwrap().as_str().unwrap()
        )
    }
}
impl std::error::Error for Error {}
pub type Result<T> = std::result::Result<T, Error>;
