#![cfg(unix)]
use rhythm::{
    execution_records::*, local_api, operation_error::Error, private_storage::Root, schedules,
};
use serde_json::{Value, json};
use std::{
    fs,
    path::PathBuf,
    process::{Child, Command, Stdio},
    time::Duration,
};
use tempfile::TempDir;

struct Fixture {
    temp: TempDir,
    root: Root,
    runtime: Child,
}
impl Fixture {
    async fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = Root::open(temp.path().join("state")).unwrap();
        let runtime = Self::spawn(&root);
        let f = Self {
            temp,
            root,
            runtime,
        };
        f.ready().await;
        f
    }
    fn spawn(root: &Root) -> Child {
        Command::new(env!("CARGO_BIN_EXE_rhythm"))
            .args([
                "--data-dir",
                root.path().to_str().unwrap(),
                "runtime",
                "run",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap()
    }
    async fn ready(&self) {
        for _ in 0..250 {
            if local_api::call(&self.root, &rhythm::runtime_protocol::Request::Status)
                .await
                .is_ok()
            {
                return;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("runtime unavailable")
    }
    fn output(&self, args: &[&str]) -> std::process::Output {
        Command::new(env!("CARGO_BIN_EXE_rhythm"))
            .args(["--json", "--data-dir", self.root.path().to_str().unwrap()])
            .args(args)
            .output()
            .unwrap()
    }
    fn cli(&self, args: &[&str]) -> Value {
        let out = self.output(args);
        assert!(
            out.status.success(),
            "synthetic CLI request failed: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        serde_json::from_slice(&out.stdout).unwrap()
    }
    fn create(&self, id: &str, script: &str, every: i64, overlap: &str, missed: &str, grace: i64) {
        let file = self.temp.path().join(format!("{id}.json"));
        fs::write(&file,json!({"id":id,"command":["/bin/sh","-c",script],"working_directory":self.temp.path(),"trigger":{"type":"interval","every_ms":every},"overlap":overlap,"missed":missed,"grace_ms":grace}).to_string()).unwrap();
        self.cli(&["schedule", "create", "--file", file.to_str().unwrap()]);
    }
    fn inspect(&self, id: &str) -> Value {
        self.cli(&["schedule", "inspect", id])["schedule"].clone()
    }
    fn history(&self, id: &str) -> Vec<Execution> {
        serde_json::from_value(self.cli(&["schedule", "history", id])["executions"].clone())
            .unwrap()
    }
    async fn running(&self, id: &str, count: usize) -> Vec<Execution> {
        for _ in 0..250 {
            let runs = self.history(id);
            if runs.len() >= count && runs[0].supervisor_pid.is_some() {
                return runs;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("scheduled execution missing")
    }
    async fn finished(&self, id: &str, count: usize) -> Vec<Execution> {
        for _ in 0..250 {
            let runs = self.history(id);
            if runs.len() >= count && runs[0].outcome.is_some() {
                return runs;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("scheduled execution did not finish")
    }
    async fn stop(&mut self, crash: bool) {
        if crash {
            self.runtime.kill().unwrap()
        } else {
            local_api::call(&self.root, &rhythm::runtime_protocol::Request::Shutdown)
                .await
                .unwrap();
        }
        wait(&mut self.runtime).await;
    }
    async fn reopen(&mut self) {
        self.runtime = Self::spawn(&self.root);
        self.ready().await;
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        if self.runtime.try_wait().unwrap().is_none() {
            let _ = self.runtime.kill();
        }
        let _ = self.runtime.wait();
    }
}
async fn wait(child: &mut Child) {
    for _ in 0..250 {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("runtime did not stop")
}
async fn file(path: PathBuf) -> String {
    for _ in 0..250 {
        if let Ok(s) = fs::read_to_string(&path)
            && !s.is_empty()
        {
            return s;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("fixture output missing")
}
fn process_running(pid: i32) -> bool {
    let out = Command::new("/bin/ps")
        .args(["-o", "stat=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    !out.stdout.is_empty() && !String::from_utf8_lossy(&out.stdout).trim().starts_with('Z')
}

#[tokio::test]
async fn cli_manual_history_logs_and_persistence() {
    let mut f = Fixture::new().await;
    f.create(
        "sample-work",
        "printf schedule-output; printf schedule-error >&2; exit 7",
        1000,
        "skip",
        "skip",
        100,
    );
    assert_eq!(f.inspect("sample-work")["enabled"], false);
    assert_eq!(
        f.cli(&["schedule", "list"])["schedules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    f.cli(&["schedule", "run-now", "sample-work"]);
    let records = f.finished("sample-work", 1).await;
    assert_eq!(records.len(), 1);
    assert_eq!(records[0].outcome, Some(Outcome::Exited { code: 7 }));
    assert!(matches!(records[0].invocation, Invocation::Manual { .. }));
    let logs = f.cli(&["schedule", "logs", &records[0].id.to_string()]);
    assert_eq!(logs["stdout"], "schedule-output");
    assert_eq!(logs["stderr"], "schedule-error");
    f.stop(false).await;
    f.reopen().await;
    assert_eq!(f.history("sample-work").len(), 1);
    assert_eq!(f.inspect("sample-work")["enabled"], false);
}

#[tokio::test]
async fn interval_resume_pause_and_queue_one_execute_without_overlapping() {
    let mut f = Fixture::new().await;
    f.create(
        "sample-interval",
        "mkdir guard || exit 99; sleep 0.3; rmdir guard; printf completed",
        150,
        "queue_one",
        "run_once",
        50,
    );
    f.cli(&["schedule", "resume", "sample-interval"]);
    let runs = f.running("sample-interval", 2).await;
    assert!(runs[1].outcome.is_some());
    assert_eq!(runs[1].outcome, Some(Outcome::Exited { code: 0 }));
    f.cli(&["schedule", "pause", "sample-interval"]);
    let runs = f.finished("sample-interval", 2).await;
    let count = runs.len();
    assert!(
        runs.iter()
            .all(|e| e.outcome == Some(Outcome::Exited { code: 0 }))
    );
    tokio::time::sleep(Duration::from_millis(400)).await;
    assert_eq!(f.history("sample-interval").len(), count);
    assert!(f.inspect("sample-interval")["pending"].is_null());
    f.stop(false).await;
}

#[tokio::test]
async fn overlap_skip_and_cancel_clean_contained_descendants() {
    let mut f = Fixture::new().await;
    f.create(
        "sample-tree",
        "trap '' TERM; sleep 30 & echo $! > child.pid; wait",
        100,
        "skip",
        "run_once",
        50,
    );
    f.cli(&["schedule", "run-now", "sample-tree"]);
    f.running("sample-tree", 1).await;
    let pid: i32 = file(f.temp.path().join("child.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    f.cli(&["schedule", "resume", "sample-tree"]);
    tokio::time::sleep(Duration::from_millis(350)).await;
    let state = f.inspect("sample-tree");
    assert!(state["pending"].is_null());
    assert_eq!(state["last_decision"]["action"], "skipped_overlap");
    assert_eq!(f.history("sample-tree").len(), 1);
    f.cli(&["schedule", "pause", "sample-tree"]);
    f.cli(&["schedule", "cancel", "sample-tree"]);
    assert_eq!(
        f.finished("sample-tree", 1).await[0].outcome,
        Some(Outcome::Stopped)
    );
    assert!(!process_running(pid));
    f.stop(false).await;
}

#[tokio::test]
async fn queued_manual_invocation_survives_runtime_crash_after_old_workload_stops() {
    let mut f = Fixture::new().await;
    f.create(
        "sample-durable",
        "echo $$ > leader.pid; exec sleep 30",
        1000,
        "queue_one",
        "run_once",
        50,
    );
    f.cli(&["schedule", "run-now", "sample-durable"]);
    let first = f.running("sample-durable", 1).await[0].clone();
    let pid: i32 = file(f.temp.path().join("leader.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    f.cli(&["schedule", "run-now", "sample-durable"]);
    f.stop(true).await;
    f.reopen().await;
    let runs = f.running("sample-durable", 2).await;
    assert_eq!(runs.len(), 2);
    assert_eq!(runs[1].id, first.id);
    assert_eq!(runs[1].outcome, Some(Outcome::Interrupted));
    assert!(!process_running(pid));
    assert!(matches!(runs[0].invocation, Invocation::Manual { .. }));
    f.cli(&["schedule", "cancel", "sample-durable"]);
    f.finished("sample-durable", 2).await;
    f.stop(false).await;
}

#[tokio::test]
async fn missed_policy_and_interval_phase_survive_real_runtime_downtime() {
    for missed in ["skip", "run_once"] {
        let mut f = Fixture::new().await;
        f.create(
            "sample-downtime",
            "printf restored",
            1200,
            "skip",
            missed,
            0,
        );
        f.cli(&["schedule", "resume", "sample-downtime"]);
        let due = f.inspect("sample-downtime")["next_run_at_ms"]
            .as_i64()
            .unwrap();
        f.stop(false).await;
        let remaining = (due + 250 - timestamp_ms()).max(0) as u64;
        tokio::time::sleep(Duration::from_millis(remaining)).await;
        f.reopen().await;
        let next = f.inspect("sample-downtime")["next_run_at_ms"]
            .as_i64()
            .unwrap();
        assert!(next > due);
        assert_eq!((next - due) % 1200, 0);
        f.cli(&["schedule", "pause", "sample-downtime"]);
        let state = f.inspect("sample-downtime");
        if missed == "skip" {
            assert!(f.history("sample-downtime").is_empty());
            assert_eq!(state["last_decision"]["action"], "skipped_missed");
        } else {
            let runs = f.finished("sample-downtime", 1).await;
            assert_eq!(runs.len(), 1);
            assert!(
                matches!(runs[0].invocation,Invocation::Scheduled{scheduled_for_ms} if scheduled_for_ms==due)
            );
        }
        f.stop(false).await;
    }
}

#[tokio::test]
async fn invalid_calendar_definition_fails_without_source_echo() {
    let f = Fixture::new().await;
    let definition = f.temp.path().join("invalid.json");
    fs::write(&definition,json!({"id":"sample-invalid","command":["/usr/bin/true"],"working_directory":"/tmp","trigger":{"type":"daily","hour":0,"minute":0,"timezone":"private-source-value"},"overlap":"skip","missed":"skip"}).to_string()).unwrap();
    let out = f.output(&["schedule", "create", "--file", definition.to_str().unwrap()]);
    assert!(!out.status.success());
    assert!(!String::from_utf8_lossy(&out.stderr).contains("private-source-value"));
    assert!(String::from_utf8_lossy(&out.stderr).contains("invalid_request"));
    assert!(matches!(
        local_api::call(
            &f.root,
            &schedules::Request::Inspect {
                id: "missing".parse().unwrap()
            }
        )
        .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn lost_monitor_blocks_queue_after_restart_and_requires_cleanup_acknowledgment() {
    let mut f = Fixture::new().await;
    f.create(
        "sample-uncertain",
        "echo $$ > leader.pid; exec sleep 30",
        1000,
        "queue_one",
        "run_once",
        50,
    );
    f.cli(&["schedule", "run-now", "sample-uncertain"]);
    let first = f.running("sample-uncertain", 1).await[0].clone();
    let pid: i32 = file(f.temp.path().join("leader.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    let out = Command::new("/bin/ps")
        .args(["-o", "pgid=", "-p", &pid.to_string()])
        .output()
        .unwrap();
    let group: i32 = String::from_utf8(out.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    struct Cleanup(i32);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            unsafe {
                libc::kill(-self.0, libc::SIGKILL);
            }
        }
    }
    let cleanup = Cleanup(group);
    f.cli(&["schedule", "run-now", "sample-uncertain"]);
    assert_eq!(
        unsafe { libc::kill(first.supervisor_pid.unwrap() as i32, libc::SIGKILL) },
        0
    );
    assert_eq!(
        f.finished("sample-uncertain", 1).await[0].outcome,
        Some(Outcome::MonitorLost)
    );
    f.stop(false).await;
    f.reopen().await;
    assert_eq!(f.history("sample-uncertain").len(), 1);
    assert_eq!(
        f.inspect("sample-uncertain")["observed"]["state"],
        "needs_attention"
    );
    let blocked = f.output(&["schedule", "run-now", "sample-uncertain"]);
    assert!(!blocked.status.success());
    assert!(String::from_utf8_lossy(&blocked.stderr).contains("recovery_required"));
    drop(cleanup);
    for _ in 0..100 {
        if !process_running(pid) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    assert!(!process_running(pid));
    let stale = f.output(&[
        "schedule",
        "acknowledge",
        "sample-uncertain",
        "--execution",
        &(first.id + 1).to_string(),
        "--confirm-cleaned",
    ]);
    assert!(!stale.status.success());
    f.cli(&[
        "schedule",
        "acknowledge",
        "sample-uncertain",
        "--execution",
        &first.id.to_string(),
        "--confirm-cleaned",
    ]);
    assert!(f.inspect("sample-uncertain")["pending"].is_null());
    assert_eq!(f.inspect("sample-uncertain")["enabled"], false);
    assert!(
        f.history("sample-uncertain")[0]
            .recovery_acknowledged_at_ms
            .is_some()
    );
    f.cli(&["schedule", "run-now", "sample-uncertain"]);
    f.running("sample-uncertain", 2).await;
    f.cli(&["schedule", "cancel", "sample-uncertain"]);
    f.finished("sample-uncertain", 2).await;
    f.stop(false).await;
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires macOS user launchd; installs and removes one disposable schedule runtime"]
async fn launchd_runtime_executes_and_restores_persisted_schedules_end_to_end() {
    let mut f = Fixture::new().await;
    struct Cleanup(Root);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = Command::new(env!("CARGO_BIN_EXE_rhythm"))
                .args([
                    "--data-dir",
                    self.0.path().to_str().unwrap(),
                    "runtime",
                    "stop",
                ])
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .status();
        }
    }
    let _cleanup = Cleanup(f.root.clone());
    f.create(
        "sample-supervised",
        "printf supervised-output",
        200,
        "skip",
        "run_once",
        100,
    );
    f.stop(false).await;
    f.cli(&["runtime", "install"]);
    f.ready().await;
    f.cli(&["schedule", "resume", "sample-supervised"]);
    f.finished("sample-supervised", 1).await;
    f.cli(&["schedule", "pause", "sample-supervised"]);
    let records = f.finished("sample-supervised", 1).await;
    assert!(
        records
            .iter()
            .all(|r| r.outcome == Some(Outcome::Exited { code: 0 }))
    );
    assert_eq!(
        f.cli(&["schedule", "logs", &records[0].id.to_string()])["stdout"],
        "supervised-output"
    );
    f.cli(&["runtime", "stop"]);
    assert!(f.root.lock().is_ok());
    f.cli(&["runtime", "install"]);
    f.ready().await;
    assert_eq!(f.history("sample-supervised").len(), records.len());
    assert_eq!(f.inspect("sample-supervised")["enabled"], false);
    f.cli(&["runtime", "stop"]);
    assert!(f.root.lock().is_ok());
}
