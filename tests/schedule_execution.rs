#![cfg(unix)]
use rhythm::{
    execution::ProcessHost,
    execution_records::*,
    private_storage::{self, Root},
    schedule_store::SqliteScheduleStore,
    schedules::{self, *},
    services::Error,
};
use serde_json::json;
use std::{cell::Cell, path::PathBuf, rc::Rc, time::Duration};
use tempfile::TempDir;

#[derive(Clone)]
struct Time(Rc<Cell<i64>>);
impl Clock for Time {
    fn now_ms(&self) -> i64 {
        self.0.get()
    }
}
type Scheduler = schedules::Manager<SqliteScheduleStore, ProcessHost, Time>;
fn open(root: &Root, time: Time) -> Scheduler {
    schedules::Manager::new(
        SqliteScheduleStore::open(root).unwrap(),
        ProcessHost::new(root.clone(), PathBuf::from(env!("CARGO_BIN_EXE_rhythm"))),
        time,
    )
    .unwrap()
}
fn create(m: &mut Scheduler, directory: &std::path::Path, id: &str, trigger: serde_json::Value) {
    m.request(Request::Create{definition:serde_json::from_value(json!({"id":id,"command":["/bin/sh","-c","printf calendar-output; exit 7"],"working_directory":directory,"trigger":trigger,"overlap":"skip","missed":"run_once"})).unwrap()}).unwrap();
}
fn id(s: &str) -> ScheduleId {
    s.parse().unwrap()
}
fn history(m: &mut Scheduler, s: &str) -> Vec<Execution> {
    match m.request(Request::History { id: id(s) }).unwrap() {
        Response::History { executions } => executions,
        _ => panic!("history"),
    }
}
async fn finish(m: &mut Scheduler, s: &str) -> Execution {
    for _ in 0..250 {
        m.tick().unwrap();
        let runs = history(m, s);
        if runs.len() == 1 && runs[0].outcome.is_some() {
            return runs[0].clone();
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("execution did not finish")
}
fn ms(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s)
        .unwrap()
        .timestamp_millis()
}

#[tokio::test]
async fn daily_and_weekly_calendar_boundaries_dispatch_real_processes_once() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    let _lock = root.lock().unwrap();
    let time = Time(Rc::new(Cell::new(ms("2024-02-28T12:00:00Z"))));
    let mut m = open(&root, time.clone());
    create(
        &mut m,
        temp.path(),
        "sample-daily",
        json!({"type":"daily","hour":9,"minute":15,"timezone":"UTC"}),
    );
    create(
        &mut m,
        temp.path(),
        "sample-weekly",
        json!({"type":"weekly","weekdays":[4],"hour":9,"minute":15,"timezone":"UTC"}),
    );
    for name in ["sample-daily", "sample-weekly"] {
        m.request(Request::Resume { id: id(name) }).unwrap();
    }
    let due = ms("2024-02-29T09:15:00Z");
    time.0.set(due - 1);
    m.tick().unwrap();
    assert!(history(&mut m, "sample-daily").is_empty());
    time.0.set(due);
    let daily = finish(&mut m, "sample-daily").await;
    let weekly = finish(&mut m, "sample-weekly").await;
    for execution in [&daily, &weekly] {
        assert_eq!(execution.outcome, Some(Outcome::Exited { code: 7 }));
        assert!(
            matches!(execution.owner,ExecutionOwner::Schedule{invocation:Invocation::Scheduled{scheduled_for_ms},..} if scheduled_for_ms==due)
        );
        assert_eq!(
            std::fs::read_to_string(root.run(execution.id).unwrap().join("stdout.log")).unwrap(),
            "calendar-output"
        );
    }
    assert_ne!(daily.id, weekly.id);
    m.tick().unwrap();
    time.0.set(due - 3600000);
    m.tick().unwrap();
    assert_eq!(history(&mut m, "sample-daily").len(), 1);
    assert_eq!(history(&mut m, "sample-weekly").len(), 1);
}

#[tokio::test]
async fn allocation_before_launch_is_recovered_using_the_same_execution_id() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    let _lock = root.lock().unwrap();
    let time = Time(Rc::new(Cell::new(1000)));
    let mut m = open(&root, time.clone());
    create(
        &mut m,
        temp.path(),
        "sample-handoff",
        json!({"type":"interval","every_ms":1000}),
    );
    m.request(Request::RunNow {
        id: id("sample-handoff"),
    })
    .unwrap();
    drop(m);
    let mut store = SqliteScheduleStore::open(&root).unwrap();
    let run = store.begin(&id("sample-handoff"), time.now_ms()).unwrap();
    drop(store);
    let mut m = open(&root, time.clone());
    m.shutdown();
    m.tick().unwrap();
    assert!(
        history(&mut m, "sample-handoff")[0]
            .supervisor_pid
            .is_none()
    );
    assert!(!root.run(run).unwrap().join("launch-attempt.json").exists());
    drop(m);
    let mut m = open(&root, time);
    let execution = finish(&mut m, "sample-handoff").await;
    assert_eq!(execution.id, run);
    assert_eq!(execution.outcome, Some(Outcome::Exited { code: 7 }));
    assert!(matches!(
        execution.owner,
        ExecutionOwner::Schedule {
            invocation: Invocation::Manual {
                requested_at_ms: 1000
            },
            ..
        }
    ));
}

#[test]
fn launch_marker_without_pid_is_uncertain_and_never_retried() {
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    let _lock = root.lock().unwrap();
    let time = Time(Rc::new(Cell::new(1000)));
    let mut m = open(&root, time.clone());
    create(
        &mut m,
        temp.path(),
        "sample-unknown",
        json!({"type":"interval","every_ms":1000}),
    );
    m.request(Request::RunNow {
        id: id("sample-unknown"),
    })
    .unwrap();
    drop(m);
    let mut store = SqliteScheduleStore::open(&root).unwrap();
    let run = store.begin(&id("sample-unknown"), time.now_ms()).unwrap();
    drop(store);
    private_storage::write_json(&root.run(run).unwrap().join("launch-attempt.json"), &true)
        .unwrap();
    let mut m = open(&root, time);
    m.tick().unwrap();
    m.tick().unwrap();
    assert_eq!(
        history(&mut m, "sample-unknown")[0].outcome,
        Some(Outcome::MonitorLost)
    );
    assert_eq!(history(&mut m, "sample-unknown").len(), 1);
    assert!(matches!(
        m.request(Request::RunNow {
            id: id("sample-unknown")
        }),
        Err(Error::RecoveryRequired)
    ));
}
