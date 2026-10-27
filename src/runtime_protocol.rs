//! Shared transport envelope. Domain requests and responses keep their own scope.
use crate::{execution_records::ExecutionId, schedules, services};
use serde::{Deserialize, Serialize, de::DeserializeOwned};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Schedule {
        command: schedules::Request,
    },
    Logs {
        execution_id: ExecutionId,
    },
    Status,
    Shutdown,
    #[serde(untagged)]
    Service(services::Request),
}
#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Response {
    Schedule {
        response: schedules::Response,
    },
    Logs {
        stdout: String,
        stderr: String,
    },
    Runtime {
        version: String,
    },
    #[serde(untagged)]
    Service(services::Response),
}
/// Each CLI command declares the response it expects while preserving the wire shape.
pub trait Command: Serialize {
    type Response: DeserializeOwned;
}
impl Command for Request {
    type Response = Response;
}
impl Command for services::Request {
    type Response = services::Response;
}
#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn envelopes_preserve_service_wire_format_and_scope_domains() {
        let request: Request = serde_json::from_value(json!({"operation":"list"})).unwrap();
        assert!(matches!(request, Request::Service(services::Request::List)));
        assert_eq!(
            serde_json::to_value(Response::Service(services::Response::Services {
                services: vec![]
            }))
            .unwrap(),
            json!({"type":"services","services":[]})
        );
        let schedule: Request =
            serde_json::from_value(json!({"operation":"schedule","command":{"operation":"list"}}))
                .unwrap();
        assert!(matches!(
            schedule,
            Request::Schedule {
                command: schedules::Request::List
            }
        ));
        assert!(
            serde_json::from_value::<services::Request>(
                json!({"operation":"schedule","command":{"operation":"list"}})
            )
            .is_err()
        );
    }
}
