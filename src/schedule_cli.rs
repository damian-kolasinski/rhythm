use crate::{
    execution_records::ExecutionId,
    local_api,
    operation_error::{Error, Result},
    private_storage::Root,
    runtime_protocol::{Request as RuntimeRequest, Response as RuntimeResponse},
    schedules::*,
};
use clap::Subcommand;
use std::path::PathBuf;
#[derive(Subcommand)]
pub enum ScheduleCommand {
    /// Create a paused schedule from a private definition file.
    Create {
        #[arg(long)]
        file: PathBuf,
    },
    List,
    Inspect {
        id: ScheduleId,
    },
    Pause {
        id: ScheduleId,
    },
    Resume {
        id: ScheduleId,
    },
    RunNow {
        id: ScheduleId,
    },
    /// Cancel the active and queued runs; future triggers remain enabled.
    Cancel {
        id: ScheduleId,
    },
    History {
        id: ScheduleId,
    },
    Logs {
        execution_id: ExecutionId,
    },
    /// Record confirmation that an uncertain execution has been manually cleaned up.
    Acknowledge {
        id: ScheduleId,
        #[arg(long)]
        execution: ExecutionId,
        #[arg(long, required = true)]
        confirm_cleaned: bool,
    },
}
pub async fn run(command: ScheduleCommand, root: Root) -> Result<RuntimeResponse> {
    let command = match command {
        ScheduleCommand::Create { file } => Request::Create {
            definition: crate::definition_file::read(&file)?,
        },
        ScheduleCommand::List => Request::List,
        ScheduleCommand::Inspect { id } => Request::Inspect { id },
        ScheduleCommand::Pause { id } => Request::Pause { id },
        ScheduleCommand::Resume { id } => Request::Resume { id },
        ScheduleCommand::RunNow { id } => Request::RunNow { id },
        ScheduleCommand::Cancel { id } => Request::Cancel { id },
        ScheduleCommand::History { id } => Request::History { id },
        ScheduleCommand::Logs { execution_id } => {
            return local_api::call(&root, &RuntimeRequest::Logs { execution_id }).await;
        }
        ScheduleCommand::Acknowledge {
            id,
            execution,
            confirm_cleaned,
        } => {
            if !confirm_cleaned {
                return Err(Error::InvalidRequest);
            }
            Request::Acknowledge {
                id,
                execution_id: execution,
            }
        }
    };
    local_api::call(&root, &RuntimeRequest::Schedule { command }).await
}
