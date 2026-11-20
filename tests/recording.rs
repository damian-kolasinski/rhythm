#![cfg(unix)]
use rhythm::{
    findings::{self, OpenSet, Said},
    jobs::Label,
    ledger::{self, Ledger},
    private_storage::Root,
    retention, run_output,
    runs::{Outcome, ResultError, RunReport, RunState, Summary, Timestamp},
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
        command
            .arg("--data-dir")
            .arg(self.state())
            .args(args)
            .env("RHYTHM_NOTIFY", "off");
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
            // Asked to stop: launchd sees the signal, the ledger knows why.
            outcome: Outcome::Stopped { signal: 15 },
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
fn keeps_the_newest_real_and_skipped_runs_and_their_output() {
    let f = Fixture::new();
    let ledger = f.ledger();
    let label: Label = LABEL.parse().unwrap();
    let run = |skipped: bool| {
        let id = ledger
            .start(&label, std::process::id(), Timestamp::now())
            .unwrap();
        let dir = ledger.output_dir(id).unwrap();
        let report = RunReport {
            skipped,
            ..RunReport::default()
        };
        ledger
            .finish(id, Timestamp::now(), Outcome::Code { code: 0 }, &report)
            .unwrap();
        dir
    };
    let first_real = run(false);
    for _ in 1..(retention::KEEP + 3) {
        run(false);
    }
    let first_skip = run(true);
    for _ in 1..(retention::KEEP_SKIPPED + 3) {
        run(true);
    }
    let runs = ledger.history(LABEL, 1000).unwrap();
    let skipped = runs.iter().filter(|r| r.state.skipped()).count();
    assert_eq!(
        (runs.len() - skipped, skipped),
        (retention::KEEP, retention::KEEP_SKIPPED)
    );
    // Real runs 1–103 and skips 104–126: the oldest of each are gone.
    assert_eq!(runs.last().unwrap().id, 4);
    assert_eq!(runs[retention::KEEP_SKIPPED - 1].id, 107);
    assert!(!first_real.exists() && !first_skip.exists());
}

#[test]
fn runs_that_never_finish_are_pruned_once_their_grace_is_over() {
    let f = Fixture::new();
    let ledger = f.ledger();
    let label: Label = LABEL.parse().unwrap();
    for _ in 0..(retention::KEEP + 3) {
        ledger.start(&label, 0, Timestamp(1_000)).unwrap();
    }
    let later = Timestamp(1_000 + retention::UNFINISHED_GRACE_MS + 1);
    let newest = ledger.start(&label, 0, later).unwrap();
    let runs = ledger.history(LABEL, 1000).unwrap();
    assert_eq!(runs.len(), retention::KEEP + 1);
    assert_eq!((runs[0].id, runs.last().unwrap().id), (newest, 4));
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

#[test]
fn notifies_when_a_job_starts_failing_and_when_it_recovers() {
    let f = Fixture::new();
    let notices = f.temp.path().join("notices");
    let notifier = f.temp.path().join("notify");
    std::fs::write(
        &notifier,
        format!(
            "#!/bin/sh\nprintf '%s | %s\\n' \"$1\" \"$2\" >> '{}'\n",
            notices.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&notifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = |code: i32| {
        f.command(&["run", LABEL, "--", "/bin/sh", "-c", &format!("exit {code}")])
            .env_remove("RHYTHM_NOTIFY")
            .env("RHYTHM_NOTIFY_COMMAND", &notifier)
            .output()
            .unwrap()
    };
    let expected = |lines: &[&str]| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let seen = std::fs::read_to_string(&notices).unwrap_or_default();
            if seen.lines().collect::<Vec<_>>() == lines {
                return;
            }
            assert!(Instant::now() < deadline, "notices: {seen:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    run(0);
    run(2);
    run(2);
    expected(&["com.example.job failed | exit 2. See rhythm logs com.example.job"]);
    run(0);
    run(0);
    expected(&[
        "com.example.job failed | exit 2. See rhythm logs com.example.job",
        "com.example.job recovered | The latest run succeeded.",
    ]);
    // Switched off in the job's environment, nothing is delivered.
    f.command(&["run", LABEL, "--", "/bin/sh", "-c", "exit 4"])
        .env("RHYTHM_NOTIFY_COMMAND", &notifier)
        .output()
        .unwrap();
    std::thread::sleep(Duration::from_millis(300));
    assert_eq!(
        std::fs::read_to_string(&notices).unwrap().lines().count(),
        2
    );
}

#[test]
fn a_missing_command_notifies_but_a_requested_stop_does_not() {
    let f = Fixture::new();
    let notices = f.temp.path().join("notices");
    let notifier = f.temp.path().join("notify");
    std::fs::write(
        &notifier,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n",
            notices.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&notifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let command = |args: &[&str]| {
        let mut command = f.command(args);
        command
            .env_remove("RHYTHM_NOTIFY")
            .env("RHYTHM_NOTIFY_COMMAND", &notifier);
        command
    };
    // Stopped by launchd while running: not the job's failure.
    let mut child = command(&[
        "run",
        LABEL,
        "--",
        "/bin/sh",
        "-c",
        "echo started; exec sleep 30",
    ])
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
    unsafe { libc::kill(child.id() as i32, libc::SIGTERM) };
    child.wait().unwrap();
    // A command that cannot start is the job's failure.
    command(&["run", LABEL, "--", "/nonexistent/program"])
        .output()
        .unwrap();
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        let seen = std::fs::read_to_string(&notices).unwrap_or_default();
        if !seen.is_empty() {
            assert_eq!(seen, "com.example.job failed\n");
            break;
        }
        assert!(Instant::now() < deadline, "no notice for a missing command");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn the_command_knows_which_job_it_runs_in() {
    let f = Fixture::new();
    f.record(&[], "echo \"$RHYTHM_JOB\"");
    assert_eq!(f.json(&["logs", LABEL])["output"], "com.example.job\n");
}

/// Writes the result file atomically, as the results guide recommends.
fn report(json: &str) -> String {
    format!(
        "[ -n \"$RHYTHM_RESULT\" ] && printf '%s' '{json}' > \"$RHYTHM_RESULT.tmp\" && mv \"$RHYTHM_RESULT.tmp\" \"$RHYTHM_RESULT\""
    )
}

fn finished_report(f: &Fixture) -> (Outcome, RunReport) {
    match f.ledger().latest(LABEL).unwrap().unwrap().state {
        RunState::Finished {
            outcome, report, ..
        } => (outcome, report),
        state => panic!("{state:?}"),
    }
}

#[test]
fn a_job_can_say_it_had_nothing_to_do() {
    let f = Fixture::new();
    let script = format!(
        "echo \"$RHYTHM_RESULT\"; {}",
        report(r#"{"version": 1, "skipped": true, "summary": "nothing new", "later": [1]}"#)
    );
    let output = f.record(&[], &script);
    assert!(output.status.success());
    let logs = f.json(&["logs", LABEL]);
    let dir = f.state().join("runs").join(logs["run"]["id"].to_string());
    assert_eq!(
        logs["output"],
        format!("{}\n", dir.join("result.json").display())
    );
    let history = f.json(&["history", LABEL]);
    let run = &history["runs"][0];
    assert_eq!(
        run["outcome"],
        serde_json::json!({"type": "code", "code": 0})
    );
    assert_eq!(run["skipped"], true);
    assert_eq!(run["summary"], "nothing new");
    assert!(run.get("result_error").is_none());
}

#[test]
fn only_a_clean_exit_can_skip_and_any_run_can_summarize() {
    let f = Fixture::new();
    let script = format!(
        "{}; exit 3",
        report(r#"{"version": 1, "skipped": true, "summary": "gave up"}"#)
    );
    assert_eq!(f.record(&[], &script).status.code(), Some(3));
    assert_eq!(
        finished_report(&f),
        (
            Outcome::Code { code: 3 },
            RunReport {
                skipped: false,
                summary: Summary::new("gave up"),
                ..RunReport::default()
            }
        )
    );
    f.record(
        &[],
        &report(r#"{"version": 1, "summary": "3 items synced"}"#),
    );
    assert_eq!(
        finished_report(&f).1.summary,
        Summary::new("3 items synced")
    );
    // Without a result file the run is recorded as before.
    f.record(&[], "exit 0");
    assert_eq!(finished_report(&f).1, RunReport::default());
}

#[test]
fn a_bad_result_file_is_noted_and_the_run_recorded_normally() {
    let f = Fixture::new();
    let elsewhere = f.temp.path().join("elsewhere.json");
    std::fs::write(&elsewhere, r#"{"version": 1, "skipped": true}"#).unwrap();
    for (script, error) in [
        (report("{not json"), ResultError::Invalid),
        (
            report(r#"{"version": 2, "skipped": true}"#),
            ResultError::UnsupportedVersion,
        ),
        (
            format!("ln -s '{}' \"$RHYTHM_RESULT\"", elsewhere.display()),
            ResultError::Unreadable,
        ),
        (
            "head -c 262145 /dev/zero | tr '\\0' ' ' > \"$RHYTHM_RESULT\"".to_owned(),
            ResultError::TooLarge,
        ),
    ] {
        assert!(f.record(&[], &script).status.success(), "{script}");
        let (outcome, report) = finished_report(&f);
        // The unread result is an open problem of its own.
        let stated = report.findings.clone().unwrap();
        assert_eq!(stated.open, 1, "{script}");
        assert_eq!(open_keys(&f.json(&["findings", LABEL])), ["rhythm:result"]);
        assert_eq!(
            (
                outcome,
                RunReport {
                    findings: None,
                    ..report
                }
            ),
            (
                Outcome::Code { code: 0 },
                RunReport {
                    result_error: Some(error),
                    ..RunReport::default()
                }
            ),
            "{script}"
        );
    }
    let history = f.json(&["history", LABEL]);
    assert_eq!(history["runs"][0]["result_error"], "too_large");
    assert_eq!(history["runs"][0]["outcome"]["code"], 0);
}

#[test]
fn only_a_stored_run_offers_a_result_file() {
    let f = Fixture::new();
    let seen = f.temp.path().join("seen");
    let script = format!("echo \"${{RHYTHM_RESULT-absent}}\" > '{}'", seen.display());
    // Recording unavailable: an inherited variable must not reach the command.
    std::fs::create_dir(f.state()).unwrap();
    std::fs::set_permissions(f.state(), std::fs::Permissions::from_mode(0o755)).unwrap();
    let output = f
        .command(&["run", LABEL, "--", "/bin/sh", "-c", &script])
        .env("RHYTHM_RESULT", "/elsewhere/result.json")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "absent\n");
    // Recorded, but without private storage for the run.
    std::fs::set_permissions(f.state(), std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::create_dir_all(f.state().join("runs")).unwrap();
    std::fs::set_permissions(
        f.state().join("runs"),
        std::fs::Permissions::from_mode(0o755),
    )
    .unwrap();
    let output = f
        .command(&["run", LABEL, "--", "/bin/sh", "-c", &script])
        .env("RHYTHM_RESULT", "/elsewhere/result.json")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert_eq!(std::fs::read_to_string(&seen).unwrap(), "absent\n");
    assert!(f.ledger().latest(LABEL).unwrap().is_some());
}

#[test]
fn skips_neither_fail_nor_recover_a_job() {
    let f = Fixture::new();
    let notices = f.temp.path().join("notices");
    let notifier = f.temp.path().join("notify");
    std::fs::write(
        &notifier,
        format!(
            "#!/bin/sh\nprintf '%s\\n' \"$1\" >> '{}'\n",
            notices.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&notifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let skip = report(r#"{"version": 1, "skipped": true}"#);
    let run = |script: &str| {
        f.command(&["run", LABEL, "--", "/bin/sh", "-c", script])
            .env_remove("RHYTHM_NOTIFY")
            .env("RHYTHM_NOTIFY_COMMAND", &notifier)
            .output()
            .unwrap()
    };
    let expected = |lines: &[&str]| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let seen = std::fs::read_to_string(&notices).unwrap_or_default();
            if seen.lines().collect::<Vec<_>>() == lines {
                return;
            }
            assert!(Instant::now() < deadline, "notices: {seen:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    run(&skip);
    run("exit 2");
    run(&skip);
    run(&skip);
    // The previous real run failed, so this failure is not news.
    run("exit 2");
    std::thread::sleep(Duration::from_millis(300));
    expected(&["com.example.job failed"]);
    run("exit 0");
    expected(&["com.example.job failed", "com.example.job recovered"]);
}

#[test]
fn history_limits_real_runs_and_logs_pass_over_skips() {
    let f = Fixture::new();
    f.record(&[], "echo first");
    f.record(&[], "echo failed; exit 1");
    for _ in 0..3 {
        f.record(
            &[],
            &format!(
                "echo skip; {}",
                report(r#"{"version": 1, "skipped": true}"#)
            ),
        );
    }
    let history = f.json(&["history", LABEL, "--limit", "1"]);
    let ids: Vec<i64> = history["runs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|r| r["id"].as_i64().unwrap())
        .collect();
    assert_eq!(ids, [5, 4, 3, 2]);
    assert_eq!(f.json(&["logs", LABEL])["output"], "failed\n");
    assert_eq!(f.json(&["logs", LABEL, "--run", "5"])["output"], "skip\n");
    let text = f.command(&["history", LABEL]).output().unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(text.contains("skipped ×3 since "), "{text}");
    let text = f.command(&["history", LABEL, "--all"]).output().unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert_eq!(text.matches("skipped").count(), 3, "{text}");
}

#[test]
fn a_version_1_ledger_is_migrated_and_a_newer_one_refused() {
    let f = Fixture::new();
    std::fs::create_dir(f.state()).unwrap();
    std::fs::set_permissions(f.state(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = f.state().join("ledger.sqlite3");
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(
        r#"CREATE TABLE runs (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             label TEXT NOT NULL,
             started_at INTEGER NOT NULL,
             recorder_pid INTEGER NOT NULL,
             ended_at INTEGER,
             outcome TEXT,
             CHECK ((ended_at IS NULL) = (outcome IS NULL))
           );
           CREATE INDEX runs_by_label ON runs (label, id);
           INSERT INTO runs (label, started_at, recorder_pid, ended_at, outcome)
             VALUES ('com.example.job', 1000, 1, 2000, '{"type":"code","code":1}');
           PRAGMA user_version = 1;"#,
    )
    .unwrap();
    drop(old);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let history = f.json(&["history", LABEL]);
    assert_eq!(history["runs"][0]["outcome"]["code"], 1);
    assert!(history["runs"][0].get("skipped").is_none());
    f.record(&[], &report(r#"{"version": 1, "skipped": true}"#));
    assert!(f.ledger().latest(LABEL).unwrap().unwrap().state.skipped());
    let version: i64 = rusqlite::Connection::open(&path)
        .unwrap()
        .query_row("PRAGMA user_version", [], |r| r.get(0))
        .unwrap();
    assert_eq!(version, 3);
    f.record(&[], &report(r#"{"version": 1, "findings": []}"#));
    assert_eq!(f.json(&["findings", LABEL])["run"], 3);

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("PRAGMA user_version = 4")
        .unwrap();
    let output = f.command(&["--json", "history", LABEL]).output().unwrap();
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "unsupported_schema");
}

fn open_keys(findings: &serde_json::Value) -> Vec<&str> {
    findings["findings"]
        .as_array()
        .unwrap()
        .iter()
        .map(|f| f["key"].as_str().unwrap())
        .collect()
}

#[test]
fn a_job_states_its_open_problems_and_rhythm_keeps_them_until_the_next_statement() {
    let f = Fixture::new();
    assert_eq!(
        f.json(&["findings", LABEL]),
        serde_json::json!({"type": "findings", "label": LABEL, "findings": []})
    );
    let ab = report(
        r#"{"version": 1, "findings": [{"key": "a", "title": "a broke", "detail": "first\nsecond", "link": "https://example.com/a", "handled_by": "someone"}, {"key": "b", "title": "b broke"}]}"#,
    );
    f.record(&[], &ab);
    let first = f.json(&["findings", LABEL]);
    assert_eq!(open_keys(&first), ["a", "b"]);
    assert_eq!(first["run"], 1);
    assert_eq!(first["findings"][0]["detail"], "first\nsecond");
    assert_eq!(first["findings"][0]["handled_by"], "someone");
    let since = first["findings"][1]["first_seen"].clone();
    assert_eq!(since, first["reported_at"]);

    // Findings count even when the run fails.
    f.record(
        &[],
        &format!(
            "{}; exit 3",
            report(r#"{"version": 1, "findings": [{"key": "b", "title": "b broke"}, {"key": "c", "title": "c broke"}]}"#)
        ),
    );
    let second = f.json(&["findings", LABEL]);
    assert_eq!(open_keys(&second), ["b", "c"]);
    assert_eq!(second["findings"][0]["first_seen"], since);
    let run = &f.json(&["history", LABEL])["runs"][0];
    assert_eq!(run["findings"]["new"], serde_json::json!(["c"]));
    assert_eq!(run["findings"]["resolved"], 1);

    // A run without a result, or without a findings list, changes nothing.
    f.record(&[], "true");
    f.record(&[], &report(r#"{"version": 1, "summary": "checked"}"#));
    assert_eq!(f.json(&["findings", LABEL])["run"], 2);

    // A result Rhythm cannot read is a problem of its own, kept with the
    // job's open ones until a readable result.
    f.record(
        &[],
        &report(r#"{"version": 1, "findings": [{"key": "rhythm:mine", "title": "t"}]}"#),
    );
    let unread = f.json(&["findings", LABEL]);
    assert_eq!(open_keys(&unread), ["rhythm:result", "b", "c"]);
    assert_eq!(
        unread["findings"][0]["title"],
        "The job's result could not be read (invalid)"
    );
    assert_eq!(
        f.json(&["history", LABEL])["runs"][0]["result_error"],
        "invalid"
    );
    f.record(&[], &report(r#"{"version": 1}"#));
    let readable = f.json(&["findings", LABEL]);
    assert_eq!(open_keys(&readable), ["b", "c"]);
    assert_eq!(readable["findings"][0]["first_seen"], since);

    f.record(&[], &report(r#"{"version": 1, "findings": []}"#));
    let text = f.command(&["findings", LABEL]).output().unwrap();
    let text = String::from_utf8(text.stdout).unwrap();
    assert!(
        text.starts_with("com.example.job has no open problems (reported by run 7, "),
        "{text}"
    );
    let history = f.command(&["history", LABEL]).output().unwrap();
    let history = String::from_utf8(history.stdout).unwrap();
    assert!(history.contains("exit 0, 2 resolved"), "{history}");
    assert!(
        history.contains("exit 3, +1 problem, 1 resolved"),
        "{history}"
    );
}

#[test]
fn notifies_when_problems_open_and_when_none_are_left_once_per_run() {
    let f = Fixture::new();
    let notices = f.temp.path().join("notices");
    let notifier = f.temp.path().join("notify");
    std::fs::write(
        &notifier,
        format!(
            "#!/bin/sh\nprintf '%s | %s\\n' \"$1\" \"$2\" >> '{}'\n",
            notices.display()
        ),
    )
    .unwrap();
    std::fs::set_permissions(&notifier, std::fs::Permissions::from_mode(0o755)).unwrap();
    let run = |keys: &[&str], code: i32| {
        let listed: Vec<String> = keys
            .iter()
            .map(|k| format!(r#"{{"key": "{k}", "title": "{k} broke"}}"#))
            .collect();
        let result = format!(r#"{{"version": 1, "findings": [{}]}}"#, listed.join(", "));
        let script = format!("{}; exit {code}", report(&result));
        f.command(&["run", LABEL, "--", "/bin/sh", "-c", &script])
            .env_remove("RHYTHM_NOTIFY")
            .env("RHYTHM_NOTIFY_COMMAND", &notifier)
            .output()
            .unwrap();
    };
    let expected = |lines: &[&str]| {
        let deadline = Instant::now() + Duration::from_secs(5);
        loop {
            let seen = std::fs::read_to_string(&notices).unwrap_or_default();
            if seen.lines().collect::<Vec<_>>() == lines {
                return;
            }
            assert!(Instant::now() < deadline, "notices: {seen:?}");
            std::thread::sleep(Duration::from_millis(20));
        }
    };
    let found =
        "com.example.job found 2 problems | a broke, +1 more. See rhythm findings com.example.job";
    let failed = "com.example.job failed | exit 2. See rhythm logs com.example.job";
    let recovered = "com.example.job recovered | The latest run succeeded.";
    let resolved = "com.example.job has no open problems | Every problem it reported is resolved.";
    run(&["a", "b"], 0);
    run(&["b", "a"], 0);
    expected(&[found]);
    // The failure is the one notice of a run that also opened a problem.
    run(&["a", "b", "c"], 2);
    expected(&[found, failed]);
    run(&["a", "b", "c"], 0);
    expected(&[found, failed, recovered]);
    run(&[], 0);
    expected(&[found, failed, recovered, resolved]);
    std::thread::sleep(Duration::from_millis(300));
    expected(&[found, failed, recovered, resolved]);
}

#[test]
fn the_run_that_stated_the_open_problems_outlives_retention() {
    let f = Fixture::new();
    f.record(
        &[],
        &report(r#"{"version": 1, "findings": [{"key": "a", "title": "a broke"}]}"#),
    );
    let ledger = f.ledger();
    let label: rhythm::jobs::Label = LABEL.parse().unwrap();
    for _ in 0..105 {
        let id = ledger.start(&label, 1, Timestamp::now()).unwrap();
        ledger
            .finish(
                id,
                Timestamp::now(),
                Outcome::Code { code: 0 },
                &RunReport::default(),
            )
            .unwrap();
    }
    let findings = f.json(&["findings", LABEL]);
    assert_eq!(
        (findings["run"].as_i64(), open_keys(&findings)),
        (Some(1), vec!["a"])
    );
    assert_eq!(
        f.json(&["history", LABEL, "--limit", "1000"])["runs"]
            .as_array()
            .unwrap()
            .len(),
        101
    );
}

#[test]
fn statements_count_in_the_order_runs_end() {
    let f = Fixture::new();
    let ledger = f.ledger();
    let label: rhythm::jobs::Label = LABEL.parse().unwrap();
    let said = |keys: &[&str]| {
        let listed: Vec<_> = keys
            .iter()
            .map(|k| serde_json::json!({"key": k, "title": format!("{k} broke")}))
            .collect();
        Said::Readable(Some(findings::parse(&serde_json::json!(listed)).unwrap()))
    };
    let finish = |id: i64, at: i64, keys: &[&str]| {
        let previous = ledger.open_findings(LABEL).unwrap();
        let open = OpenSet::after(previous.as_ref().map(|s| &s.set), said(keys), Timestamp(at));
        ledger
            .finish_stating(
                id,
                Timestamp(at),
                Outcome::Code { code: 0 },
                &RunReport::default(),
                open.as_ref(),
            )
            .unwrap();
    };
    // A long run starts first; a quick one starts and ends inside it.
    let long = ledger.start(&label, 1, Timestamp(1000)).unwrap();
    let quick = ledger.start(&label, 1, Timestamp(2000)).unwrap();
    finish(quick, 3000, &["a"]);
    finish(long, 4000, &[]);
    let open = ledger.open_findings(LABEL).unwrap().unwrap();
    assert_eq!(
        (open.run, open.set.open.len(), open.set.resolved),
        (long, 0, 1)
    );
}

#[test]
fn a_version_2_ledger_gains_findings_and_a_damaged_statement_counts_as_none() {
    let f = Fixture::new();
    std::fs::create_dir(f.state()).unwrap();
    std::fs::set_permissions(f.state(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = f.state().join("ledger.sqlite3");
    let old = rusqlite::Connection::open(&path).unwrap();
    old.execute_batch(
        r#"CREATE TABLE runs (
             id INTEGER PRIMARY KEY AUTOINCREMENT,
             label TEXT NOT NULL,
             started_at INTEGER NOT NULL,
             recorder_pid INTEGER NOT NULL,
             ended_at INTEGER,
             outcome TEXT,
             CHECK ((ended_at IS NULL) = (outcome IS NULL))
           );
           CREATE INDEX runs_by_label ON runs (label, id);
           ALTER TABLE runs ADD COLUMN skipped INTEGER NOT NULL DEFAULT 0;
           ALTER TABLE runs ADD COLUMN summary TEXT;
           ALTER TABLE runs ADD COLUMN result_error TEXT;
           INSERT INTO runs (label, started_at, recorder_pid, ended_at, outcome, summary)
             VALUES ('com.example.job', 1000, 1, 2000, '{"type":"code","code":0}', 'kept');
           PRAGMA user_version = 2;"#,
    )
    .unwrap();
    drop(old);
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    assert_eq!(f.json(&["history", LABEL])["runs"][0]["summary"], "kept");
    f.record(
        &[],
        &report(r#"{"version": 1, "findings": [{"key": "a", "title": "a broke"}]}"#),
    );
    assert_eq!(open_keys(&f.json(&["findings", LABEL])), ["a"]);

    rusqlite::Connection::open(&path)
        .unwrap()
        .execute_batch("UPDATE runs SET findings = '{broken' WHERE id = 2")
        .unwrap();
    assert_eq!(open_keys(&f.json(&["findings", LABEL])), Vec::<&str>::new());
    let history = f.json(&["history", LABEL]);
    assert_eq!(history["runs"].as_array().unwrap().len(), 2);
    assert!(history["runs"][0].get("findings").is_none());
}
