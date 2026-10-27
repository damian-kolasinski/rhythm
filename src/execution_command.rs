//! Validated foreground execution input shared by services and schedules.
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Input")]
pub struct CommandSpec {
    command: Vec<String>,
    working_directory: PathBuf,
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn execution_input_rejects_relative_paths_nuls_and_oversized_arguments() {
        assert!(CommandSpec::new(vec!["/bin/sh".into(), "".into()], "/tmp".into()).is_ok());
        for (command, directory) in [
            (vec![], "/tmp"),
            (vec!["relative".into()], "/tmp"),
            (vec!["/bin/sh".into(), "hidden\0value".into()], "/tmp"),
            (vec!["/bin/sh".into()], "relative"),
            (vec!["/bin/sh".into(), "x".repeat(65536)], "/tmp"),
            (vec!["/bin/sh".into(); 257], "/tmp"),
        ] {
            assert!(CommandSpec::new(command, directory.into()).is_err());
        }
    }
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    command: Vec<String>,
    working_directory: PathBuf,
}
impl TryFrom<Input> for CommandSpec {
    type Error = &'static str;
    fn try_from(v: Input) -> Result<Self, Self::Error> {
        Self::new(v.command, v.working_directory)
    }
}
impl CommandSpec {
    pub fn new(command: Vec<String>, working_directory: PathBuf) -> Result<Self, &'static str> {
        if command.is_empty()
            || command.len() > 256
            || !Path::new(&command[0]).is_absolute()
            || command.iter().any(|s| s.contains('\0'))
            || command.iter().map(String::len).sum::<usize>() > 65536
            || !working_directory.is_absolute()
            || working_directory
                .as_os_str()
                .as_encoded_bytes()
                .contains(&0)
        {
            return Err("invalid_command");
        }
        Ok(Self {
            command,
            working_directory,
        })
    }
    pub fn command(&self) -> &[String] {
        &self.command
    }
    pub fn working_directory(&self) -> &Path {
        &self.working_directory
    }
}
