//! Run read-only Rhythm commands on another machine over ssh.
use crate::{
    bounded_command::{self, Failure},
    operation_error::{Error, Result},
};
use serde::de::DeserializeOwned;
use std::{path::PathBuf, process::Command, str::FromStr, time::Duration};

const MAX_OUTPUT: u64 = 16 * 1024 * 1024;
/// Non-interactive ssh shells rarely load the profile that puts user tools on PATH.
const REMOTE_PATH: &str = r#"PATH="$HOME/.local/share/mise/shims:$HOME/.local/bin:/opt/homebrew/bin:/usr/local/bin:$PATH""#;

/// An ssh destination, as in `ssh_config`; never an option.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Host(String);
impl Host {
    pub fn as_str(&self) -> &str {
        &self.0
    }
}
impl FromStr for Host {
    type Err = Error;
    fn from_str(name: &str) -> Result<Self> {
        let valid = (1..=253).contains(&name.len())
            && !name.starts_with('-')
            && name
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"._-@".contains(&b));
        valid
            .then(|| Self(name.to_owned()))
            .ok_or(Error::InvalidRequest)
    }
}

pub struct Ssh {
    program: PathBuf,
    timeout: Duration,
}
impl Ssh {
    pub fn system() -> Self {
        Self::at_boundary("/usr/bin/ssh".into(), Duration::from_secs(30))
    }
    pub fn at_boundary(program: PathBuf, timeout: Duration) -> Self {
        Self { program, timeout }
    }
    /// Runs `rhythm --json ARGS` remotely and decodes its response, which a
    /// current Rhythm prints even when it exits 1 with failures inside.
    pub fn call<T: DeserializeOwned>(&self, host: &Host, args: &[&str]) -> Result<T> {
        let remote = std::iter::once(format!("{REMOTE_PATH} exec rhythm --json"))
            .chain(args.iter().map(|a| quote(a)))
            .collect::<Vec<_>>()
            .join(" ");
        let mut command = Command::new(&self.program);
        command
            .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-T", "--"])
            .arg(host.as_str())
            .arg(remote);
        let output = match bounded_command::run(command, self.timeout, MAX_OUTPUT) {
            Ok(output) => output,
            Err(Failure::TooLarge) => return Err(Error::RemoteIncompatible),
            Err(_) => return Err(Error::RemoteUnavailable),
        };
        match output.status.code() {
            // ssh reserves 255 for its own connection and authentication failures.
            Some(255) | None => Err(Error::RemoteUnavailable),
            Some(code) => serde_json::from_slice(&output.stdout).map_err(|_| {
                match code {
                    0 => None,
                    _ => remote_error(&output.stderr),
                }
                .unwrap_or(Error::RemoteIncompatible)
            }),
        }
    }
}

/// A current Rhythm reports its own structured error; anything else is incompatible.
fn remote_error(stderr: &[u8]) -> Option<Error> {
    #[derive(serde::Deserialize)]
    struct Failure {
        error: Code,
    }
    #[derive(serde::Deserialize)]
    struct Code {
        code: Error,
    }
    let line = stderr.split(|b| *b == b'\n').find(|l| !l.is_empty())?;
    let failure: Failure = serde_json::from_slice(line).ok()?;
    (failure.error.code != Error::InvalidRequest).then_some(failure.error.code)
}

fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', r"'\''"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_ssh_config_names_and_refuses_options() {
        for name in ["devbox", "build-01.example.com", "user@host", "10.0.0.2"] {
            assert!(name.parse::<Host>().is_ok(), "{name}");
        }
        for name in ["", "-oProxyCommand=x", "host name", "host;rm", "a$b", "x\n"] {
            assert!(name.parse::<Host>().is_err(), "{name:?}");
        }
    }

    #[test]
    fn quotes_arguments_for_the_remote_shell() {
        assert_eq!(quote("it's"), r"'it'\''s'");
    }

    #[test]
    fn reads_remote_errors_but_treats_unknown_commands_as_incompatible() {
        assert_eq!(
            remote_error(br#"{"error":{"code":"unsupported_platform","message":"x"}}"#),
            Some(Error::UnsupportedPlatform)
        );
        assert_eq!(
            remote_error(br#"{"error":{"code":"invalid_request","message":"x"}}"#),
            None
        );
        assert_eq!(remote_error(b"zsh: command not found: rhythm"), None);
    }
}
