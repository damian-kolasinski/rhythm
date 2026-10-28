//! Noninteractive runtime lifecycle commands.
use crate::{
    local_api,
    operation_error::{Error, Result},
    private_storage::Root,
    runtime,
    runtime_protocol::{Request as RuntimeRequest, Response as RuntimeResponse},
    runtime_supervision::RuntimeSupervisor,
};
use clap::Subcommand;

#[derive(Subcommand)]
pub enum RuntimeCommand {
    Run,
    Status,
    Stop,
    Install,
    Uninstall,
}
pub async fn runtime(command: RuntimeCommand, root: Root) -> Result<RuntimeResponse> {
    match command {
        RuntimeCommand::Run => {
            runtime::run(
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
