#![cfg(unix)]
use rhythm::{
    execution::ProcessHost,
    private_storage::{self, Root},
    service_store::SqliteStore,
    services::*,
};
use serde_json::json;
use std::{path::PathBuf, time::Duration};
use tempfile::TempDir;

fn definition(id: &str, directory: &std::path::Path) -> Definition {
    serde_json::from_value(json!({"id":id,"command":["/usr/bin/true"],"working_directory":directory,"restart":"never","restart_delay_ms":100})).unwrap()
}
fn manager(root: &Root) -> Manager<SqliteStore, ProcessHost> {
    Manager::new(
        SqliteStore::open(root).unwrap(),
        ProcessHost::new(root.clone(), PathBuf::from(env!("CARGO_BIN_EXE_rhythm"))),
    )
    .unwrap()
}
fn history(manager: &mut Manager<SqliteStore, ProcessHost>, id: &ServiceId) -> Vec<Execution> {
    match manager
        .request(Request::History { id: id.clone() })
        .unwrap()
    {
        Response::History { executions } => executions,
        _ => panic!("history"),
    }
}

#[test]
fn recovered_completion_honors_never_policy_without_repeating_execution() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    let _lock = root.lock().unwrap();
    let d = definition("sample-recovered", temp.path());
    let mut store = SqliteStore::open(&root).unwrap();
    store.create(&d).unwrap();
    store.request_start(d.id()).unwrap();
    let run = store.begin(d.id(), timestamp_ms()).unwrap();
    private_storage::write_json(
        &root.run(run).unwrap().join("result.json"),
        &Completion {
            outcome: Outcome::Exited { code: 0 },
            finished_at_ms: timestamp_ms(),
        },
    )
    .unwrap();
    drop(store);
    let mut manager = manager(&root);
    manager.tick().unwrap();
    manager.tick().unwrap();
    let records = history(&mut manager, d.id());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Some(Outcome::Exited { code: 0 }));
}

#[test]
fn launch_attempt_without_recorded_pid_blocks_pending_restart() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    let _lock = root.lock().unwrap();
    let d = definition("sample-unknown", temp.path());
    let mut store = SqliteStore::open(&root).unwrap();
    store.create(&d).unwrap();
    store.request_start(d.id()).unwrap();
    let run = store.begin(d.id(), timestamp_ms()).unwrap();
    store.request_start(d.id()).unwrap();
    private_storage::write_json(&root.run(run).unwrap().join("launch-attempt.json"), &true)
        .unwrap();
    drop(store);
    let mut manager = manager(&root);
    manager.tick().unwrap();
    manager.tick().unwrap();
    let records = history(&mut manager, d.id());
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Some(Outcome::MonitorLost));
    assert!(matches!(
        manager.request(Request::Restart { id: d.id().clone() }),
        Err(Error::RecoveryRequired)
    ));
}

#[tokio::test]
async fn durable_start_intent_and_never_attempted_execution_resume_after_reopen() {
    for preallocated in [false, true] {
        let temp = TempDir::new().unwrap();
        let root = Root::open(temp.path().join("state")).unwrap();
        let _lock = root.lock().unwrap();
        let d = definition("sample-intent", temp.path());
        let mut store = SqliteStore::open(&root).unwrap();
        store.create(&d).unwrap();
        store.request_start(d.id()).unwrap();
        if preallocated {
            store.begin(d.id(), timestamp_ms()).unwrap();
        }
        drop(store);
        let mut manager = manager(&root);
        let expected = if preallocated { 2 } else { 1 };
        let mut finished = false;
        for _ in 0..150 {
            manager.tick().unwrap();
            let records = history(&mut manager, d.id());
            if records.len() == expected && records[0].outcome == Some(Outcome::Exited { code: 0 })
            {
                if preallocated {
                    assert_eq!(records[1].outcome, Some(Outcome::Interrupted));
                }
                finished = true;
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        assert!(
            finished,
            "durable start intent was lost (preallocated={preallocated}): {:?}",
            history(&mut manager, d.id())
        );
    }
}
