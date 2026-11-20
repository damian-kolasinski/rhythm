#![cfg(unix)]
use rhythm::{
    jobs::{Activation, Exit, Health, Observation, Supervisor},
    launchd::Launchd,
    operation_error::Error,
    remote::Ssh,
    status::{self, HostOutcome, JobReport},
    targets::Target,
};
use std::{fs, path::Path, time::Duration};
use tempfile::TempDir;

mod command_fixture;

const LAUNCHCTL: &str = r#"#!/bin/sh
set -eu
cd "$(dirname "$0")"
printf '%s\n' "$*" >> launchctl-calls
[ "$1" = print ] || exit 64
file="observed/$(printf '%s' "$2" | tr / _)"
[ -f "$file" ] || exit 113
cat "$file"
"#;

struct Fixture {
    temp: TempDir,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        fs::create_dir_all(temp.path().join("agents")).unwrap();
        fs::create_dir_all(temp.path().join("observed")).unwrap();
        command_fixture::write(&temp.path().join("launchctl"), LAUNCHCTL);
        Self { temp }
    }
    fn path(&self) -> &Path {
        self.temp.path()
    }
    fn define(&self, file: &str, label: &str, body: &str) {
        let program = if body.contains("ProgramArguments") {
            ""
        } else {
            "<key>Program</key><string>/bin/true</string>"
        };
        fs::write(
            self.path().join("agents").join(file),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string>{program}{body}</dict></plist>"#
            ),
        )
        .unwrap();
    }
    fn observe(&self, domain: &str, label: &str, fields: &str) {
        fs::write(
            self.path()
                .join("observed")
                .join(format!("{domain}_501_{label}")),
            format!("{domain}/501/{label} = {{\n{fields}}}\n"),
        )
        .unwrap();
    }
    fn launchd(&self) -> Launchd {
        Launchd::at_boundary(
            self.path().join("agents"),
            self.path().join("launchctl"),
            501,
        )
    }
    fn report(&self) -> Vec<JobReport> {
        status::jobs(&self.launchd(), &|_| None).unwrap()
    }
}

#[test]
fn reports_every_defined_job_most_urgent_first() {
    let f = Fixture::new();
    f.define(
        "com.example.report.plist",
        "com.example.report",
        "<key>StartCalendarInterval</key><dict><key>Hour</key><integer>9</integer><key>Minute</key><integer>0</integer></dict>",
    );
    f.observe(
        "gui",
        "com.example.report",
        "\truns = 4\n\tlast exit code = 0\n",
    );
    f.define(
        "com.example.scan.plist",
        "com.example.scan",
        "<key>StartInterval</key><integer>300</integer>",
    );
    f.observe(
        "gui",
        "com.example.scan",
        "\truns = 19\n\tlast exit code = 2\n",
    );
    f.define(
        "com.example.server.plist",
        "com.example.server",
        "<key>KeepAlive</key><true/>",
    );
    // A headless machine keeps background agents in the user domain.
    f.observe(
        "user",
        "com.example.server",
        "\tstate = running\n\truns = 3\n\tpid = 812\n\tlast terminating signal = Killed: 9\n",
    );
    f.define(
        "com.example.idle.plist",
        "com.example.idle",
        "<key>KeepAlive</key><true/>",
    );
    f.observe(
        "gui",
        "com.example.idle",
        "\truns = 1\n\tlast exit code = 0\n",
    );
    f.define("com.example.disabled.plist", "com.example.disabled", "");
    fs::write(
        f.path().join("agents/com.example.broken.plist"),
        "{not a plist",
    )
    .unwrap();
    fs::write(
        f.path().join("agents/com.example.placeholder.plist"),
        r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict/></plist>"#,
    )
    .unwrap();
    fs::write(f.path().join("agents/notes.txt"), "ignored").unwrap();
    fs::write(f.path().join("agents/.hidden.plist"), "ignored").unwrap();

    let reports = f.report();
    let summary: Vec<(&str, Health)> = reports
        .iter()
        .map(|r| (r.label.as_str(), r.health))
        .collect();
    assert_eq!(
        summary,
        [
            ("com.example.broken", Health::Invalid),
            ("com.example.scan", Health::Failing),
            ("com.example.idle", Health::Down),
            ("com.example.server", Health::Running),
            ("com.example.report", Health::Ok),
            ("com.example.disabled", Health::Unloaded),
        ]
    );
    assert_eq!(
        reports[1].observation,
        Observation::Loaded {
            pid: None,
            runs: 19,
            last_exit: Some(Exit::Code { code: 2 })
        }
    );
    assert_eq!(
        reports[3].activation,
        Some(Activation::Service { always: true })
    );
    assert_eq!(reports[0].activation, None);
}

#[test]
fn recorded_jobs_only_observes_jobs_using_the_rhythm_recorder() {
    let fixture = Fixture::new();
    for label in ["dev.rhythm.sample", "com.example.adopted"] {
        fixture.define(
            &format!("{label}.plist"),
            label,
            &format!("<key>ProgramArguments</key><array><string>/example/bin/rhythm</string><string>run</string><string>{label}</string><string>--</string><string>/bin/true</string></array>"),
        );
    }
    fixture.define("ordinary.plist", "com.example.ordinary", "");
    fixture.observe(
        "gui",
        "dev.rhythm.sample",
        "\truns = 1\n\tlast exit code = 3\n",
    );
    fixture.observe(
        "gui",
        "com.example.adopted",
        "\truns = 1\n\tlast exit code = 0\n",
    );
    let jobs = fixture.launchd().recorded_jobs().unwrap();
    assert_eq!(
        jobs.iter()
            .map(|job| job.label.as_str())
            .collect::<Vec<_>>(),
        ["com.example.adopted", "dev.rhythm.sample"]
    );
    let calls = fs::read_to_string(fixture.path().join("launchctl-calls")).unwrap();
    assert!(!calls.contains("com.example.ordinary"));
}

#[test]
fn json_contract_round_trips_for_remote_hosts() {
    let f = Fixture::new();
    f.define("a.plist", "com.example.a", "<key>RunAtLoad</key><true/>");
    f.observe(
        "gui",
        "com.example.a",
        "\truns = 1\n\tlast terminating signal = Terminated: 15\n",
    );
    let reports = f.report();
    let json = serde_json::to_value(&reports[0]).unwrap();
    assert_eq!(
        json,
        serde_json::json!({
            "label": "com.example.a",
            "health": "failing",
            "activation": {"type": "triggered", "triggers": [{"type": "at_load"}]},
            "observation": {"state": "loaded", "runs": 1, "last_exit": {"type": "signal", "signal": 15}},
            "recorded": false
        })
    );
    assert_eq!(
        serde_json::from_value::<JobReport>(json).unwrap(),
        reports[0]
    );
}

#[test]
fn a_missing_agents_directory_has_no_jobs() {
    let f = Fixture::new();
    fs::remove_dir(f.path().join("agents")).unwrap();
    assert!(f.report().is_empty());
}

#[test]
fn an_unrunnable_launchctl_is_reported() {
    let f = Fixture::new();
    f.define("a.plist", "com.example.a", "");
    fs::remove_file(f.path().join("launchctl")).unwrap();
    assert_eq!(
        status::jobs(&f.launchd(), &|_| None),
        Err(Error::SupervisorUnavailable)
    );
}

const SSH: &str = r#"#!/bin/sh
cd "$(dirname "$0")"
printf '%s\n' "$@" > "ssh-$7-args"
case "$7" in
  good) cat remote-response ;;
  down) exit 255 ;;
  old) echo '{"error":{"code":"invalid_request","message":"Invalid command."}}' >&2; exit 1 ;;
  linux) echo '{"type":"status","hosts":[{"host":"local","error":"unsupported_platform"}]}'; exit 1 ;;
  lying) sed 's/"health":"ok"/"health":"failing"/' remote-response ;;
  garbage) echo 'Welcome!'; exit 0 ;;
  slow) sleep 5 ;;
esac
"#;

#[test]
fn gathers_hosts_concurrently_and_keeps_going_past_failures() {
    let f = Fixture::new();
    f.define("a.plist", "com.example.a", "<key>RunAtLoad</key><true/>");
    f.observe("gui", "com.example.a", "\truns = 1\n\tlast exit code = 0\n");
    let remote_jobs = f.report();
    fs::write(
        f.path().join("remote-response"),
        serde_json::to_string(&status::StatusResponse {
            hosts: vec![status::HostStatus {
                host: "local".into(),
                outcome: HostOutcome::Jobs {
                    jobs: remote_jobs.clone(),
                },
            }],
        })
        .unwrap(),
    )
    .unwrap();
    command_fixture::write(&f.path().join("ssh"), SSH);
    let ssh = Ssh::at_boundary(f.path().join("ssh"), Duration::from_millis(1500));
    let targets: Vec<Target> = [
        "local", "good", "lying", "down", "old", "linux", "garbage", "slow",
    ]
    .into_iter()
    .map(|h| Target::parse(h).unwrap())
    .collect();
    let started = std::time::Instant::now();
    let response = status::gather(&targets, || Ok(vec![]), &ssh);
    assert!(
        started.elapsed() < Duration::from_secs(4),
        "hosts were queried serially"
    );
    let outcomes: Vec<(&str, &HostOutcome)> = response
        .hosts
        .iter()
        .map(|h| (h.host.as_str(), &h.outcome))
        .collect();
    assert_eq!(
        outcomes,
        [
            ("local", &HostOutcome::Jobs { jobs: vec![] }),
            (
                "good",
                &HostOutcome::Jobs {
                    jobs: remote_jobs.clone()
                }
            ),
            // Health is recomputed from the reported facts, not trusted.
            ("lying", &HostOutcome::Jobs { jobs: remote_jobs }),
            (
                "down",
                &HostOutcome::Failed {
                    error: Error::RemoteUnavailable
                }
            ),
            (
                "old",
                &HostOutcome::Failed {
                    error: Error::RemoteIncompatible
                }
            ),
            (
                "linux",
                &HostOutcome::Failed {
                    error: Error::UnsupportedPlatform
                }
            ),
            (
                "garbage",
                &HostOutcome::Failed {
                    error: Error::RemoteIncompatible
                }
            ),
            (
                "slow",
                &HostOutcome::Failed {
                    error: Error::RemoteUnavailable
                }
            ),
        ]
    );
    let args = fs::read_to_string(f.path().join("ssh-good-args")).unwrap();
    let args: Vec<&str> = args.lines().collect();
    assert_eq!(
        args[..6],
        ["-o", "BatchMode=yes", "-o", "ConnectTimeout=10", "-T", "--"]
    );
    assert!(
        args[7].ends_with("exec rhythm --json 'status'"),
        "{}",
        args[7]
    );
}

#[test]
fn host_names_cannot_become_ssh_options() {
    assert_eq!(
        Target::parse("-oProxyCommand=x"),
        Err(Error::InvalidRequest)
    );
    assert_eq!(Target::parse("local"), Ok(Target::Local));
}

#[test]
fn unreadable_definitions_are_reported_and_fifos_are_skipped() {
    let f = Fixture::new();
    let fifo = f.path().join("agents/com.example.pipe.plist");
    assert!(
        std::process::Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    let started = std::time::Instant::now();
    let reports = f.report();
    assert!(started.elapsed() < Duration::from_secs(2));
    assert_eq!(
        reports
            .iter()
            .map(|r| (r.label.as_str(), r.health))
            .collect::<Vec<_>>(),
        [("com.example.pipe", Health::Invalid)]
    );
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(f.path().join("agents"), fs::Permissions::from_mode(0o000)).unwrap();
    let result = status::jobs(&f.launchd(), &|_| None);
    fs::set_permissions(f.path().join("agents"), fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(result, Err(Error::DefinitionsUnavailable));
}

fn binary(home: &str, args: &[&str]) -> std::process::Output {
    std::process::Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .args(args)
        .env("HOME", home)
        .output()
        .unwrap()
}

#[test]
fn host_failures_are_reported_on_stdout_and_exit_1() {
    // A relative HOME cannot locate LaunchAgents, so the local host fails.
    let output = binary("relative", &["--json", "status"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stderr.is_empty());
    let response: status::StatusResponse = serde_json::from_slice(&output.stdout).unwrap();
    assert!(matches!(
        response.hosts[0].outcome,
        HostOutcome::Failed {
            error: Error::InvalidRequest | Error::UnsupportedPlatform
        }
    ));
}

#[test]
fn invalid_requests_are_reported_on_stderr() {
    let output = binary("/", &["--json", "status", "--host", "-oProxyCommand=x"]);
    assert_eq!(output.status.code(), Some(1));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "invalid_request");
    let output = binary("/", &["status", "--host", "-oProxyCommand=x"]);
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "Invalid command. Run rhythm --help.\n"
    );
}

#[test]
fn recorded_jobs_carry_their_newest_run() {
    use rhythm::{
        jobs::Label,
        jobs::RunStore,
        ledger::{self, Ledger},
        private_storage::Root,
        runs::{Outcome, RunReport, Timestamp},
    };
    let f = Fixture::new();
    let state = f.path().join("state").display().to_string();
    let wrapped = |label: &str| {
        format!(
            "<key>ProgramArguments</key><array><string>/opt/bin/rhythm</string><string>--data-dir</string><string>{state}</string><string>run</string><string>{label}</string><string>--</string><string>/bin/true</string></array>"
        )
    };
    f.define(
        "a.plist",
        "com.example.recorded",
        &wrapped("com.example.recorded"),
    );
    f.observe(
        "gui",
        "com.example.recorded",
        "\truns = 2\n\tlast exit code = 1\n",
    );
    f.define("b.plist", "com.example.plain", "");
    f.observe(
        "gui",
        "com.example.plain",
        "\truns = 2\n\tlast exit code = 0\n",
    );
    let state = f.path().join("state");
    let ledger = Ledger::open(Root::open(state.clone()).unwrap(), ledger::READER_WAIT).unwrap();
    for label in ["com.example.recorded", "com.example.plain"] {
        let label: Label = label.parse().unwrap();
        let id = ledger.start(&label, 1, Timestamp(1000)).unwrap();
        ledger
            .finish(
                id,
                Timestamp(2000),
                Outcome::Code { code: 1 },
                &RunReport::default(),
            )
            .unwrap();
    }
    drop(ledger);
    let requested = std::cell::RefCell::new(vec![]);
    let reports = status::jobs(&f.launchd(), &|store| {
        requested.borrow_mut().push(store.clone());
        let RunStore::Directory(dir) = store else {
            return None;
        };
        Ledger::open_existing(Root::open(dir.clone()).unwrap()).unwrap()
    })
    .unwrap();
    // Each job's own state directory is read, once.
    assert_eq!(*requested.borrow(), [RunStore::Directory(state)]);
    assert_eq!(reports[0].label, "com.example.recorded");
    assert!(reports[0].recorded);
    assert_eq!(reports[0].last_run.as_ref().unwrap().id, 1);
    // History of a job that no longer records is not attributed to it.
    assert!(!reports[1].recorded);
    assert_eq!(reports[1].last_run, None);
}

#[test]
fn a_recorded_job_fails_until_a_real_run_succeeds_however_often_it_skips() {
    use rhythm::jobs::RunStore;
    use rhythm::{
        jobs::Label,
        ledger::{self, Ledger},
        private_storage::Root,
        runs::{Outcome, RunReport, Timestamp, Verdict},
    };
    let f = Fixture::new();
    let state = f.path().join("state");
    f.define(
        "a.plist",
        "com.example.poll",
        &format!(
            "<key>ProgramArguments</key><array><string>/opt/bin/rhythm</string><string>--data-dir</string><string>{}</string><string>run</string><string>com.example.poll</string><string>--</string><string>/bin/true</string></array>",
            state.display()
        ),
    );
    // launchd saw the skip exit 0.
    f.observe(
        "gui",
        "com.example.poll",
        "\truns = 4\n\tlast exit code = 0\n",
    );
    let ledger = Ledger::open(Root::open(state.clone()).unwrap(), ledger::READER_WAIT).unwrap();
    let label: Label = "com.example.poll".parse().unwrap();
    let run = |code, skipped| {
        let id = ledger.start(&label, 1, Timestamp(1000)).unwrap();
        let report = RunReport {
            skipped,
            ..RunReport::default()
        };
        ledger
            .finish(id, Timestamp(2000), Outcome::Code { code }, &report)
            .unwrap();
    };
    let ledgers = |_: &RunStore| Ledger::open_existing(Root::open(state.clone()).unwrap()).unwrap();
    let report = || status::jobs(&f.launchd(), &ledgers).unwrap().remove(0);
    run(0, false);
    run(1, false);
    run(0, true);
    run(0, true);
    let failing = report();
    assert_eq!(failing.health, Health::Failing);
    assert_eq!(failing.verdict, Some(Verdict::Failed));
    assert_eq!(failing.last_run.as_ref().unwrap().id, 2);
    assert_eq!(failing.last_skip.as_ref().unwrap().id, 4);
    // Another machine's report is judged by the same facts, and only a
    // recorded job by its recorded verdict.
    let mut claimed = serde_json::to_value(&failing).unwrap();
    assert_eq!(claimed["verdict"], "failed");
    claimed["health"] = "ok".into();
    let mut unrecorded = claimed.clone();
    unrecorded["label"] = "com.example.plain".into();
    unrecorded["recorded"] = false.into();
    fs::write(
        f.path().join("remote-response"),
        serde_json::json!({
            "type": "status",
            "hosts": [{"host": "local", "jobs": [claimed, unrecorded]}]
        })
        .to_string(),
    )
    .unwrap();
    command_fixture::write(&f.path().join("ssh"), SSH);
    let ssh = Ssh::at_boundary(f.path().join("ssh"), Duration::from_secs(5));
    let response = status::gather(&[Target::parse("good").unwrap()], || Ok(vec![]), &ssh);
    let HostOutcome::Jobs { jobs } = &response.hosts[0].outcome else {
        panic!("{response:?}")
    };
    let health: Vec<Health> = jobs.iter().map(|j| j.health).collect();
    assert_eq!(health, [Health::Failing, Health::Ok]);
    run(0, false);
    let recovered = report();
    assert_eq!(recovered.health, Health::Ok);
    assert_eq!(recovered.last_run.as_ref().unwrap().id, 5);
    assert_eq!(recovered.last_skip, None);
    // launchd saw a failure after the newest recorded run exited 0: the
    // recorder could not record that run, and the failure still shows.
    f.observe(
        "gui",
        "com.example.poll",
        "\truns = 6\n\tlast exit code = 7\n",
    );
    assert_eq!(report().health, Health::Failing);
    // A stop after a good run is launchd's signal, not a failure.
    let id = ledger.start(&label, 1, Timestamp(3000)).unwrap();
    ledger
        .finish(
            id,
            Timestamp(4000),
            Outcome::Stopped { signal: 15 },
            &RunReport::default(),
        )
        .unwrap();
    f.observe(
        "gui",
        "com.example.poll",
        "\truns = 7\n\tlast terminating signal = Terminated: 15\n",
    );
    assert_eq!(report().health, Health::Ok);
}
