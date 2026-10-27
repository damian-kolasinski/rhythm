use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

pub use crate::workload_id::WorkloadId as ServiceId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Restart {
    Never,
    OnFailure,
    Always,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "DefinitionInput")]
pub struct Definition {
    id: ServiceId,
    #[serde(flatten)]
    command_spec: crate::execution_command::CommandSpec,
    restart: Restart,
    restart_delay_ms: u64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct DefinitionInput {
    id: ServiceId,
    command: Vec<String>,
    working_directory: PathBuf,
    restart: Restart,
    #[serde(default = "default_delay")]
    restart_delay_ms: u64,
}
fn default_delay() -> u64 {
    1000
}
impl TryFrom<DefinitionInput> for Definition {
    type Error = &'static str;
    fn try_from(v: DefinitionInput) -> Result<Self, Self::Error> {
        if !(100..=60000).contains(&v.restart_delay_ms) {
            return Err("invalid_definition");
        }
        let command_spec =
            crate::execution_command::CommandSpec::new(v.command, v.working_directory)?;
        Ok(Self {
            id: v.id,
            command_spec,
            restart: v.restart,
            restart_delay_ms: v.restart_delay_ms,
        })
    }
}
impl Definition {
    pub fn id(&self) -> &ServiceId {
        &self.id
    }
    pub fn command(&self) -> &[String] {
        self.command_spec.command()
    }
    pub fn working_directory(&self) -> &Path {
        self.command_spec.working_directory()
    }
    pub fn command_spec(&self) -> &crate::execution_command::CommandSpec {
        &self.command_spec
    }
    pub fn restart(&self) -> Restart {
        self.restart
    }
    pub fn restart_delay_ms(&self) -> u64 {
        self.restart_delay_ms
    }
    pub fn should_restart(&self, outcome: &super::Outcome) -> bool {
        !matches!(
            outcome,
            super::Outcome::MonitorLost | super::Outcome::OutputUnavailable
        ) && match self.restart {
            Restart::Never => false,
            Restart::Always => true,
            Restart::OnFailure => !matches!(outcome, super::Outcome::Exited { code: 0 }),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn validates_definition_at_deserialization_boundary() {
        let base = serde_json::json!({"id":"sample-worker","command":["/bin/true"],"working_directory":"/tmp","restart":"never"});
        let d: Definition = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(d.restart_delay_ms(), 1000);
        for (key, value) in [
            ("id", serde_json::json!("../escape")),
            ("command", serde_json::json!(["relative"])),
            ("working_directory", serde_json::json!("relative")),
            ("restart_delay_ms", serde_json::json!(0)),
            ("environment", serde_json::json!({"SECRET":"private"})),
        ] {
            let mut v = base.clone();
            v[key] = value;
            assert!(serde_json::from_value::<Definition>(v).is_err());
        }
    }
    #[test]
    fn explicit_restart_policy_never_overrides_uncertainty() {
        use super::super::Outcome;
        for (policy, zero, failure) in [
            ("never", false, false),
            ("on_failure", false, true),
            ("always", true, true),
        ] {
            let d: Definition = serde_json::from_value(serde_json::json!({"id":"sample-policy","command":["/bin/true"],"working_directory":"/tmp","restart":policy})).unwrap();
            assert_eq!(d.should_restart(&Outcome::Exited { code: 0 }), zero);
            for outcome in [
                Outcome::Exited { code: 7 },
                Outcome::Signalled { signal: 15 },
                Outcome::SpawnFailed,
                Outcome::Interrupted,
                Outcome::Stopped,
            ] {
                assert_eq!(d.should_restart(&outcome), failure);
            }
            assert!(!d.should_restart(&Outcome::MonitorLost));
            assert!(!d.should_restart(&Outcome::OutputUnavailable));
        }
    }
}
