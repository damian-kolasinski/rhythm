//! Local runtime controls and schedule commands share one flat transport.
use crate::{execution_records::ExecutionId, schedules};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Logs {
        execution_id: ExecutionId,
    },
    Status,
    Shutdown,
    #[serde(untagged)]
    Schedule(schedules::Request),
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Logs {
        stdout: String,
        stderr: String,
    },
    Runtime {
        version: String,
    },
    #[serde(untagged)]
    Schedule(schedules::Response),
}
pub trait Command: Serialize {
    type Response: DeserializeOwned;
}
impl Command for Request {
    type Response = Response;
}
impl Command for schedules::Request {
    type Response = schedules::Response;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn schedule_commands_use_one_flat_protocol_without_removed_operations() {
        let schedule: Request = serde_json::from_value(json!({"operation":"list"})).unwrap();
        assert!(matches!(
            schedule,
            Request::Schedule(schedules::Request::List)
        ));
        assert_eq!(
            serde_json::to_value(Response::Schedule(schedules::Response::Schedules {
                schedules: vec![]
            }))
            .unwrap(),
            json!({"type":"schedules","schedules":[]})
        );
        for operation in [
            "start", "stop", "restart", "service", "discover", "schedule",
        ] {
            assert!(serde_json::from_value::<Request>(json!({"operation":operation})).is_err());
        }
        assert!(serde_json::from_value::<Request>(json!({"operation":"create","definition":{"id":"sample","command":["/bin/true"],"working_directory":"/tmp","restart":"always"}})).is_err());
    }
}
