#![cfg(unix)]
use rhythm::{
    jobs::Label,
    ledger::{self, Ledger},
    private_storage::Root,
    run_output,
    runs::{Outcome, RunState, Timestamp},
};
use std::{
    os::unix::{
        fs::PermissionsExt,
        process::{CommandExt, ExitStatusExt},
    },
    path::PathBuf,
    process::{Command, Output, Stdio},
    time::{Duration, Instant},
};
use tempfile::TempDir;

const LABEL: &str = "com.example.job";

struct Fixture {
    temp: TempDir,
}
impl Fixture {
    fn new() -> Self {
        Self {
            temp: TempDir::new().unwrap(),
        }
    }
    fn state(&self) -> PathBuf {
        self.temp.path().join("state")
    }
    fn command(&self, args: &[&str]) -> Command {
        let mut command = Command::new(env!("CARGO_BIN_EXE_rhythm"));
        command.arg("--data-dir").arg(self.state()).args(args);
        command
    }
    fn record(&self, extra: &[&str], script: &str) -> Output {
        let mut args = vec!["run", LABEL];
        args.extend_from_slice(extra);
        args.extend(["--", "/bin/sh", "-c", script]);
        self.command(&args).output().unwrap()
    }
    fn json(&self, args: &[&str]) -> serde_json::Value {
        let mut all = vec!["--json"];
        all.extend_from_slice(args);
        let output = self.command(&all).output().unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn ledger(&self) -> Ledger {
        Ledger::open(Root::open(self.state()).unwrap(), ledger::READER_WAIT).unwrap()
    }
}

#[test]
fn records_outcome_and_output_and_exits_like_the_command() {
    let f = Fixture::new();
    // stdout and stderr are separate pipes; a pause keeps their order certain.
    let output = f.record(&[], "echo out; sleep 0.1; echo err >&2; exit 3");
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty() && output.stderr.is_empty());
    let run = f.ledger().latest(LABEL).unwrap().unwrap();
    assert!(matches!(
        run.state,
        RunState::Finished {
            outcome: Outcome::Code { code: 3 },
            ..
        }
    ));
    let logs = f.json(&["logs", LABEL]);
    assert_eq!(logs["output"], "out\nerr\n");
    assert_eq!(logs["run"]["id"], run.id);
    assert_eq!(logs["truncated"], false);
}

#[test]
fn forwards_output_to_the_original_streams_when_asked() {
    let f = Fixture::new();
    let output = f.record(&["--forward"], "echo out; sleep 0.1; echo err >&2");
    assert!(output.status.success());
    assert_eq!(output.stdout, b"out\n");
    assert_eq!(output.stderr, b"err\n");
    assert_eq!(f.json(&["logs", LABEL])["output"], "out\nerr\n");
}

#[test]
fn the_command_keeps_launchds_environment_and_directory() {
    let f = Fixture::new();
    let output = f
        .command(&[
            "run",
            LABEL,
            "--",
            "/bin/sh",
            "-c",
            "echo \"$SAMPLE_SETTING $HOME\"; pwd",
        ])
        .env("SAMPLE_SETTING", "kept")
        .env("HOME", "/sample/home")
        .current_dir("/usr")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(
        f.json(&["logs", LABEL])["output"],
        "kept /sample/home\n/usr\n"
    );
}

#[test]
fn termination_reaches_the_command_and_the_recorder_dies_the_same_way() {
    let f = Fixture::new();
    let mut child = f
        .command(&[
            "run",
            LABEL,
            "--",
            "/bin/sh",
            "-c",
            "echo started; exec sleep 30",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f
        .state()
        .join("runs/1/output.log")
        .metadata()
        .is_ok_and(|m| m.len() > 0)
    {
        assert!(Instant::now() < deadline, "the command never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = child.wait().unwrap();
    assert_eq!(status.signal(), Some(libc::SIGTERM));
    assert!(matches!(
        f.ledger().latest(LABEL).unwrap().unwrap().state,
        RunState::Finished {
            outcome: Outcome::Signal { signal: 15 },
            ..
        }
    ));
}

#[test]
fn a_missing_command_is_recorded_as_not_started() {
    let f = Fixture::new();
    let output = f
        .command(&["run", LABEL, "--", "/nonexistent/program"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(127));
    assert!(matches!(
        f.ledger().latest(LABEL).unwrap().unwrap().state,
        RunState::Finished {
            outcome: Outcome::SpawnFailed,
            ..
        }
    ));
}

#[test]
fn an_unusable_ledger_never_stops_the_job() {
    let f = Fixture::new();
    std::fs::create_dir(f.state()).unwrap();
    std::fs::set_permissions(f.state(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = f.record(&[], "echo ran; exit 4");
    assert_eq!(output.status.code(), Some(4));
    assert_eq!(output.stdout, b"ran\n");
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "rhythm: this run cannot be recorded (insecure_storage); running the command anyway\n"
    );
    let output = f
        .command(&["run", "bad label", "--", "/bin/sh", "-c", "exit 5"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(5));
}

#[test]
fn descendants_outlive_the_recorder_and_their_output_is_kept() {
    let f = Fixture::new();
    let marker = f.temp.path().join("marker");
    let started = Instant::now();
    let script = format!(
        "(sleep 3; echo late; touch '{}') & echo done",
        marker.display()
    );
    let output = f.record(&[], &script);
    assert!(output.status.success());
    assert!(
        started.elapsed() < Duration::from_secs(3),
        "the recorder waited for descendants"
    );
    let deadline = Instant::now() + Duration::from_secs(10);
    while !marker.exists() {
        assert!(
            Instant::now() < deadline,
            "a descendant died when the recorder exited"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let deadline = Instant::now() + Duration::from_secs(5);
    while f.json(&["logs", LABEL])["output"] != "done\nlate\n" {
        assert!(
            Instant::now() < deadline,
            "the drainer did not keep late output"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
}

#[test]
fn a_stop_before_the_command_starts_prevents_the_run() {
    let f = Fixture::new();
    let ledger = f.ledger();
    let blocker = rusqlite::Connection::open(f.state().join("ledger.sqlite3")).unwrap();
    blocker.execute_batch("BEGIN IMMEDIATE").unwrap();
    let marker = f.temp.path().join("ran");
    let mut child = f
        .command(&[
            "run",
            LABEL,
            "--",
            "/usr/bin/touch",
            marker.to_str().unwrap(),
        ])
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    let status = child.wait().unwrap();
    blocker.execute_batch("ROLLBACK").unwrap();
    drop(ledger);
    assert_eq!(status.signal(), Some(libc::SIGTERM));
    assert!(!marker.exists());
}

#[test]
fn a_run_whose_output_cannot_be_stored_is_still_recorded() {
    let f = Fixture::new();
    std::fs::create_dir_all(f.state().join("runs")).unwrap();
    std::fs::set_permissions(f.state(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::set_permissions(
        f.state().join("runs"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let output = f.record(&[], "echo unseen; exit 0");
    assert!(output.status.success());
    assert!(matches!(
        f.ledger().latest(LABEL).unwrap().unwrap().state,
        RunState::Finished {
            outcome: Outcome::Code { code: 0 },
            ..
        }
    ));
}

#[test]
fn a_killed_recorder_leaves_a_lost_run() {
    let f = Fixture::new();
    let mut child = f
        .command(&[
            "run",
            LABEL,
            "--",
            "/bin/sh",
            "-c",
            "echo started; exec sleep 30",
        ])
        .process_group(0)
        .stdout(Stdio::null())
        .spawn()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(10);
    while !f
        .state()
        .join("runs/1/output.log")
        .metadata()
        .is_ok_and(|m| m.len() > 0)
    {
        assert!(Instant::now() < deadline, "the command never started");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        f.ledger().latest(LABEL).unwrap().unwrap().state,
        RunState::Running
    );
    let group = child.id() as i32;
    unsafe { libc::kill(child.id() as i32, libc::SIGKILL) };
    child.wait().unwrap();
    assert_eq!(
        f.ledger().latest(LABEL).unwrap().unwrap().state,
        RunState::Lost
    );
    unsafe { libc::kill(-group, libc::SIGKILL) };
}

#[cfg(target_os = "macos")]
#[test]
fn a_terminal_interrupt_reaches_the_command_once() {
    let f = Fixture::new();
    let count = f.temp.path().join("count");
    let script = format!(
        "trap 'echo int >> {count}; exit 0' INT; echo ready; while :; do sleep 0.05; done",
        count = count.display()
    );
    let mut terminal = Command::new("/usr/bin/script")
        .args([
            "-q",
            "/dev/null",
            env!("CARGO_BIN_EXE_rhythm"),
            "--data-dir",
        ])
        .arg(f.state())
        .args(["run", LABEL, "--forward", "--", "/bin/sh", "-c", &script])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .unwrap();
    let mut stdout = terminal.stdout.take().unwrap();
    let mut seen = Vec::new();
    while !String::from_utf8_lossy(&seen).contains("ready") {
        let mut byte = [0u8; 64];
        let read = std::io::Read::read(&mut stdout, &mut byte).unwrap();
        assert!(read > 0, "the command never started");
        seen.extend_from_slice(&byte[..read]);
    }
    std::io::Write::write_all(terminal.stdin.as_mut().unwrap(), b"\x03").unwrap();
    terminal.wait().unwrap();
    std::thread::sleep(Duration::from_millis(200));
    assert_eq!(std::fs::read_to_string(&count).unwrap(), "int\n");
}

#[test]
fn concurrent_recorders_share_the_ledger() {
    let f = Fixture::new();
    let children: Vec<_> = (0..8)
        .map(|i| {
            f.command(&["run", LABEL, "--", "/bin/sh", "-c", &format!("exit {i}")])
                .spawn()
                .unwrap()
        })
        .collect();
    for child in children {
        child.wait_with_output().unwrap();
    }
    let history = f.json(&["history", LABEL]);
    let mut codes: Vec<i64> = history["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["outcome"]["code"].as_i64().unwrap())
        .collect();
    codes.sort();
    assert_eq!(codes, (0..8).collect::<Vec<_>>());
}

#[test]
fn keeps_the_newest_runs_and_their_output() {
    let f = Fixture::new();
    let ledger = f.ledger();
    let label: Label = LABEL.parse().unwrap();
    let mut first_dir = None;
    for _ in 0..(ledger::KEEP + 3) {
        let id = ledger
            .start(&label, std::process::id(), Timestamp::now())
            .unwrap();
        let dir = ledger.output_dir(id).unwrap();
        first_dir.get_or_insert(dir);
        ledger
            .finish(id, Timestamp::now(), Outcome::Code { code: 0 })
            .unwrap();
    }
    let runs = ledger.history(LABEL, 1000).unwrap();
    assert_eq!(runs.len() as i64, ledger::KEEP);
    assert_eq!(runs.last().unwrap().id, 4);
    assert!(!first_dir.unwrap().exists());
}

#[test]
fn a_recorder_that_vanished_leaves_a_lost_run() {
    let f = Fixture::new();
    let ledger = f.ledger();
    let mut gone = Command::new("/usr/bin/true").spawn().unwrap();
    let pid = gone.id();
    gone.wait().unwrap();
    let label: Label = LABEL.parse().unwrap();
    ledger.start(&label, pid, Timestamp::now()).unwrap();
    assert_eq!(ledger.latest(LABEL).unwrap().unwrap().state, RunState::Lost);
    ledger
        .start(&label, std::process::id(), Timestamp::now())
        .unwrap();
    assert_eq!(
        ledger.latest(LABEL).unwrap().unwrap().state,
        RunState::Running
    );
}

#[test]
fn readers_report_missing_runs_without_creating_state() {
    let f = Fixture::new();
    assert_eq!(f.json(&["history", LABEL])["runs"], serde_json::json!([]));
    let output = f.command(&["--json", "logs", LABEL]).output().unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "not_found");
    assert!(!f.state().exists());

    f.record(&[], "echo mine");
    let output = f
        .command(&["--json", "logs", "com.example.other", "--run", "1"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let output = f
        .command(&["--json", "logs", LABEL, "--bytes", "0"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
}

#[test]
fn logs_return_the_newest_bytes_and_say_so() {
    let f = Fixture::new();
    f.record(&[], "printf 'first-part second-part'");
    let logs = f.json(&["logs", LABEL, "--bytes", "11"]);
    assert_eq!(logs["output"], "second-part");
    assert_eq!(logs["truncated"], true);
    let segment = run_output::SEGMENT.to_string();
    f.record(
        &[],
        &format!("head -c {segment} /dev/zero | tr '\\0' a; echo; echo last"),
    );
    let logs = f.json(&["logs", LABEL, "--bytes", "5"]);
    assert_eq!(logs["output"], "last\n");
}
