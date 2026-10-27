//! Noninteractive service commands. All operations go through the runtime API.
use crate::{
    local_api,
    private_storage::Root,
    runtime_protocol::{Request as RuntimeRequest, Response as RuntimeResponse},
    service_runtime,
    services::*,
};
use clap::Subcommand;
use std::path::PathBuf;

#[derive(Subcommand)]
pub enum RuntimeCommand {
    Run,
    Status,
    Stop,
    Install,
    Uninstall,
}
#[derive(Subcommand)]
pub enum ServiceCommand {
    Create {
        #[arg(long)]
        file: PathBuf,
    },
    List,
    Inspect {
        id: ServiceId,
    },
    Start {
        id: ServiceId,
    },
    Stop {
        id: ServiceId,
    },
    Restart {
        id: ServiceId,
    },
    /// Resume control only after manually cleaning up an uncertain execution.
    Acknowledge {
        id: ServiceId,
        #[arg(long)]
        execution: ExecutionId,
        #[arg(long, required = true)]
        confirm_cleaned: bool,
    },
    History {
        id: ServiceId,
    },
    Logs {
        execution_id: ExecutionId,
    },
}
pub fn root(data_dir: Option<PathBuf>) -> Result<Root> {
    let path = match data_dir {
        Some(path) => path,
        None => {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .filter(|p| p.is_absolute())
                .ok_or(Error::InvalidRequest)?;
            if cfg!(target_os = "macos") {
                home.join("Library/Application Support/Rhythm")
            } else {
                home.join(".local/state/rhythm")
            }
        }
    };
    Root::open(path)
}
pub async fn runtime(command: RuntimeCommand, root: Root) -> Result<RuntimeResponse> {
    match command {
        RuntimeCommand::Run => {
            service_runtime::run(
                root,
                std::env::current_exe().map_err(|_| Error::ExecutionUnavailable)?,
            )
            .await?;
            Ok(RuntimeResponse::Runtime {
                version: env!("CARGO_PKG_VERSION").into(),
            })
        }
        RuntimeCommand::Status => local_api::call(&root, &RuntimeRequest::Status).await,
        RuntimeCommand::Stop => {
            if cfg!(target_os = "macos") {
                crate::launchd_runtime::LaunchdRuntime::current_user()?.uninstall(root.path())?;
            }
            stop_foreground(&root).await
        }
        RuntimeCommand::Install => {
            crate::launchd_runtime::LaunchdRuntime::current_user()?.install(
                &std::env::current_exe().map_err(|_| Error::ExecutionUnavailable)?,
                root.path(),
            )?;
            Ok(RuntimeResponse::Runtime {
                version: env!("CARGO_PKG_VERSION").into(),
            })
        }
        RuntimeCommand::Uninstall => {
            crate::launchd_runtime::LaunchdRuntime::current_user()?.uninstall(root.path())?;
            Ok(RuntimeResponse::Runtime {
                version: env!("CARGO_PKG_VERSION").into(),
            })
        }
    }
}
/// A removed registration does not prove that a foreground runtime has exited.
pub async fn stop_foreground(root: &Root) -> Result<RuntimeResponse> {
    match local_api::call(root, &RuntimeRequest::Shutdown).await {
        Ok(_) | Err(Error::RuntimeUnavailable) => {}
        Err(e) => return Err(e),
    }
    for _ in 0..250 {
        match root.lock() {
            Ok(_) => {
                return Ok(RuntimeResponse::Runtime {
                    version: env!("CARGO_PKG_VERSION").into(),
                });
            }
            Err(Error::RuntimeAlreadyRunning) => {
                tokio::time::sleep(std::time::Duration::from_millis(20)).await
            }
            Err(e) => return Err(e),
        }
    }
    Err(Error::RuntimeUnavailable)
}
pub async fn service(command: ServiceCommand, root: Root) -> Result<RuntimeResponse> {
    let request = match command {
        ServiceCommand::Create { file } => Request::Create {
            definition: crate::definition_file::read(&file)?,
        },
        ServiceCommand::List => Request::List,
        ServiceCommand::Inspect { id } => Request::Inspect { id },
        ServiceCommand::Start { id } => Request::Start { id },
        ServiceCommand::Stop { id } => Request::Stop { id },
        ServiceCommand::Restart { id } => Request::Restart { id },
        ServiceCommand::Acknowledge {
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
        ServiceCommand::History { id } => Request::History { id },
        ServiceCommand::Logs { execution_id } => {
            return local_api::call(&root, &RuntimeRequest::Logs { execution_id }).await;
        }
    };
    local_api::call(&root, &request)
        .await
        .map(RuntimeResponse::Service)
}
