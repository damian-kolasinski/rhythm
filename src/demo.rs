//! Disposable synthetic work, exercised through the real application and runtime.
use crate::{
    browser_launch,
    execution::ProcessHost,
    operation_error::{Error, Result},
    private_storage::Root,
    schedule_store::SqliteScheduleStore,
    schedules,
};
use std::{path::Path, time::Duration};
pub async fn run(json: bool, no_open: bool) -> Result<()> {
    let temp = tempfile::tempdir().map_err(|_| Error::StorageUnavailable)?;
    let root = Root::open(temp.path().join("state"))?;
    let executable = std::env::current_exe().map_err(|_| Error::ExecutionUnavailable)?;
    seed(&root, &executable)?;
    let runtime = crate::runtime::run_presented(root.clone(), executable, true);
    tokio::pin!(runtime);
    let ready = tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Ok(info) = browser_launch::prepare(&root).await {
                return info;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    });
    let launch = tokio::select! {result=&mut runtime=>return result,launch=ready=>launch};
    let delivered = match launch {
        Ok(launch) => browser_launch::deliver(&launch, json, no_open),
        Err(_) => Err(Error::RuntimeUnavailable),
    };
    if let Err(error) = delivered {
        let shutdown = crate::local_api::call(&root, &crate::runtime_protocol::Request::Shutdown);
        let _ = tokio::join!(shutdown, &mut runtime);
        return Err(error);
    }

    runtime.await
}

fn seed(root: &Root, executable: &Path) -> Result<()> {
    let _lock = root.lock()?;
    let mut schedules = schedules::Manager::new(
        SqliteScheduleStore::open(root)?,
        ProcessHost::new(root.clone(), executable.to_path_buf()),
        schedules::SystemClock,
    )?;
    for (id, trigger, enabled, script) in [
        (
            "sample-report",
            serde_json::json!({"type":"interval","every_ms":30000}),
            true,
            "printf 'Sample task completed.\\n'",
        ),
        (
            "sample-backup",
            serde_json::json!({"type":"weekly","weekdays":[1,5],"hour":9,"minute":15,"timezone":"UTC"}),
            false,
            "printf 'Sample task completed.\\n'",
        ),
        (
            "sample-cleanup",
            serde_json::json!({"type":"daily","hour":9,"minute":15,"timezone":"UTC"}),
            true,
            "printf 'Sample task completed.\\n'",
        ),
        (
            "sample-export",
            serde_json::json!({"type":"interval","every_ms":60000}),
            false,
            "printf 'Export destination unavailable.\\n' >&2; exit 4",
        ),
    ] {
        let definition=serde_json::from_value(serde_json::json!({"id":id,"command":["/bin/sh","-c",script],"working_directory":"/tmp","trigger":trigger,"overlap":"skip","missed":"run_once"})).unwrap();
        schedules.request(schedules::Request::Create { definition })?;
        if id == "sample-export" {
            schedules.request(schedules::Request::RunNow {
                id: id.parse().unwrap(),
            })?;
        }
        if enabled {
            schedules.request(schedules::Request::Resume {
                id: id.parse().unwrap(),
            })?;
        }
    }
    schedules.request(schedules::Request::RunNow {
        id: "sample-report".parse().unwrap(),
    })?;
    Ok(())
}
