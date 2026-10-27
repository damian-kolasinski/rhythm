use super::Trigger;
use crate::{execution_command::CommandSpec, workload_id::WorkloadId};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
pub type ScheduleId = WorkloadId;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Overlap {
    Skip,
    QueueOne,
}
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Missed {
    Skip,
    RunOnce,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Input")]
pub struct Definition {
    id: ScheduleId,
    #[serde(flatten)]
    command: CommandSpec,
    trigger: Trigger,
    overlap: Overlap,
    missed: Missed,
    grace_ms: i64,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Input {
    id: ScheduleId,
    command: Vec<String>,
    working_directory: PathBuf,
    trigger: Trigger,
    overlap: Overlap,
    missed: Missed,
    #[serde(default = "default_grace")]
    grace_ms: i64,
}
fn default_grace() -> i64 {
    1000
}
impl TryFrom<Input> for Definition {
    type Error = &'static str;
    fn try_from(v: Input) -> Result<Self, Self::Error> {
        if !(0..=3_600_000).contains(&v.grace_ms) {
            return Err("invalid_grace");
        }
        Ok(Self {
            id: v.id,
            command: CommandSpec::new(v.command, v.working_directory)?,
            trigger: v.trigger,
            overlap: v.overlap,
            missed: v.missed,
            grace_ms: v.grace_ms,
        })
    }
}
impl Definition {
    pub fn id(&self) -> &ScheduleId {
        &self.id
    }
    pub fn command(&self) -> &CommandSpec {
        &self.command
    }
    pub fn trigger(&self) -> &Trigger {
        &self.trigger
    }
    pub fn overlap(&self) -> Overlap {
        self.overlap
    }
    pub fn missed(&self) -> Missed {
        self.missed
    }
    pub fn grace_ms(&self) -> i64 {
        self.grace_ms
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn definitions_require_explicit_policies_and_validate_grace() {
        let base = json!({"id":"sample-schedule","command":["/usr/bin/true"],"working_directory":"/tmp","trigger":{"type":"interval","every_ms":1000},"overlap":"skip","missed":"skip"});
        let d: Definition = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(d.grace_ms(), 1000);
        for grace in [0, 3600000] {
            let mut v = base.clone();
            v["grace_ms"] = json!(grace);
            assert!(serde_json::from_value::<Definition>(v).is_ok());
        }
        for (field, value) in [
            ("grace_ms", json!(-1)),
            ("grace_ms", json!(3600001)),
            ("overlap", json!("parallel")),
            ("missed", json!("replay_all")),
            ("restart", json!("always")),
        ] {
            let mut v = base.clone();
            v[field] = value;
            assert!(serde_json::from_value::<Definition>(v).is_err());
        }
        for field in ["missed", "overlap"] {
            let mut v = base.clone();
            v.as_object_mut().unwrap().remove(field);
            assert!(serde_json::from_value::<Definition>(v).is_err());
        }
    }
}
