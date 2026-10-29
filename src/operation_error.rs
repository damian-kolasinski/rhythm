use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    InvalidRequest,
    NotFound,
    JobNotFound,
    JobNotLoaded,
    AlreadyExists,
    Busy,
    RestartRequired,
    NotRecorded,
    NotOwned,
    UnsupportedDefinition,
    TimedOut,
    Refused,
    RollbackIncomplete,
    StorageUnavailable,
    InsecureStorage,
    UnsupportedSchema,
    DefinitionsUnavailable,
    UnsupportedPlatform,
    SupervisorUnavailable,
    RemoteUnavailable,
    RemoteIncompatible,
}
impl Error {
    /// A fixed explanation that never echoes paths, values, or OS diagnostics.
    pub fn message(self) -> &'static str {
        match self {
            Self::InvalidRequest => "Invalid command. Run rhythm --help.",
            Self::NotFound => "No recorded run matches.",
            Self::JobNotFound => "No launchd job has this label.",
            Self::JobNotLoaded => "The job is not loaded into launchd.",
            Self::AlreadyExists => "A job with this label already exists.",
            Self::Busy => "The job is running now.",
            Self::RestartRequired => "The job is running. Pass --restart to stop and reload it.",
            Self::NotRecorded => "The job does not record its runs.",
            Self::NotOwned => "Rhythm only removes jobs it created with `rhythm add`.",
            Self::UnsupportedDefinition => {
                "The job's definition uses a form Rhythm cannot change safely."
            }
            Self::Refused => "launchd refused the job's definition; nothing was changed.",
            Self::RollbackIncomplete => {
                "launchd refused the change, and the previous job could not be fully restored. Check it with `rhythm status`."
            }
            Self::TimedOut => "The run did not finish in time; it continues in the background.",
            Self::StorageUnavailable => "Rhythm's private storage is unavailable.",
            Self::InsecureStorage => "Rhythm's private storage is accessible to other users.",
            Self::UnsupportedSchema => {
                "Rhythm's run ledger was written by an incompatible version."
            }
            Self::DefinitionsUnavailable => "The LaunchAgents directory cannot be read.",
            Self::UnsupportedPlatform => "This command needs macOS launchd.",
            Self::SupervisorUnavailable => "launchctl did not answer.",
            Self::RemoteUnavailable => "The host did not answer over ssh.",
            Self::RemoteIncompatible => {
                "Rhythm on the host is missing or incompatible with this version."
            }
        }
    }
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
