use serde::{Deserialize, Serialize};
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Error {
    InvalidRequest,
    NotFound,
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
