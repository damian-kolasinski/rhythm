#![cfg(unix)]
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::{Command, Output},
};

const RHYTHM: &str = env!("CARGO_BIN_EXE_rhythm");
const JOB: &str = "com.example.session-review";

struct Fixture {
    tmp: tempfile::TempDir,
}

impl Fixture {
    fn new() -> Self {
        Self {
            tmp: tempfile::tempdir().unwrap(),
        }
    }
    fn path(&self, path: &str) -> PathBuf {
        self.tmp.path().join(path)
    }
    fn cmd(&self, args: &[&str]) -> Command {
        let mut cmd = Command::new(RHYTHM);
        cmd.arg("--data-dir")
            .arg(self.path("state"))
            .args(args)
            .env("RHYTHM_NOTIFY", "off");
        cmd
    }
    fn json(&self, args: &[&str]) -> Value {
        let output = self.cmd(args).arg("--json").output().unwrap();
        parse(output)
    }
    fn launcher(&self, script: &str) -> PathBuf {
        let file = self.path("launcher");
        fs::write(&file, format!("#!/bin/sh\nset -eu\n{script}")).unwrap();
        fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
        file
    }
    fn configure(&self, backend: &str, executable: &Path, flags: &[&str]) {
        let mut args = vec![
            "session",
            "configure",
            "--backend",
            backend,
            "--executable",
            executable.to_str().unwrap(),
        ];
        args.extend_from_slice(flags);
        assert_eq!(self.json(&args)["backend"], backend);
    }
    fn record(&self, script: &str) -> Output {
        self.cmd(&[
            "run",
            JOB,
            "--forward",
            "--",
            "/bin/sh",
            "-eu",
            "-c",
            script,
            "test",
        ])
        .arg(RHYTHM)
        .arg(self.tmp.path())
        .output()
        .unwrap()
    }
}

fn parse(output: Output) -> Value {
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn t3_launches_worktree_sessions_and_they_outlive_the_run() {
    let f = Fixture::new();
    let launcher = f.launcher(r#"
case "$2" in
start)
    [ "$1" = thread ]
    [ "$3" = --base-dir ]
    [ "$9" = origin/main ]
    [ "$RHYTHM_SESSION_TEST" = yes ]
    [ -z "${RHYTHM_RUN_ID-}" ]
    prompt=$(cat)
    case "$prompt" in *'Record the outcome in Rhythm'*) ;; *) exit 3 ;; esac
    echo launch >> "$0.calls"
    printf '{"threadId":"synthetic-id"}\n'
    ;;
show)
    printf '{"thread":{"threadId":"synthetic-id","status":"idle","pendingRequestCount":1,"link":"https://example.com/threads/synthetic-id"},"messages":[]}\n'
    ;;
esac
"#);
    let t3_home = f.path("t3");
    f.configure(
        "t3",
        &launcher,
        &["--t3-data-dir", t3_home.to_str().unwrap()],
    );
    let job = r#"
export RHYTHM_SESSION_TEST=yes
for key in first second first; do
    printf 'Review the synthetic fixture.' |
        "$1" session spawn --key "$key" --title 'Review fixture' --repo "$2"
done
printf '{"version":1,"skipped":true}' > "$RHYTHM_RESULT"
"#;
    let launch = f.record(job);
    assert!(
        launch.status.success(),
        "{}",
        String::from_utf8_lossy(&launch.stderr)
    );
    let output = String::from_utf8(launch.stdout).unwrap();
    let ids: Vec<_> = output.lines().collect();
    assert_eq!(ids.len(), 3);
    assert_eq!(ids[0], ids[2]);
    assert_ne!(ids[0], ids[1]);
    assert_eq!(
        fs::read_to_string(f.path("launcher.calls")).unwrap(),
        "launch\nlaunch\n"
    );

    let history = f.json(&["history", JOB]);
    let run = &history["runs"][0];
    assert_eq!(run["session_spawns"]["count"], 2);
    assert!(run.get("skipped").is_none());
    let run = run["id"].as_i64().unwrap().to_string();
    let sessions = f.json(&["session", "list", "--run", &run]);
    assert_eq!(sessions[0]["launch"]["provider_id"], "synthetic-id");
    let shown = f.json(&["session", "show", ids[0]]);
    assert_eq!(shown["activity"], "waiting");
    assert_eq!(
        shown["launch"]["link"],
        "https://example.com/threads/synthetic-id"
    );
    assert_eq!(shown["outcome"], Value::Null);
    let pr = "https://example.com/owner/project/pull/42";
    let report = f.json(&[
        "session",
        "report",
        ids[0],
        "--state",
        "done",
        "--summary",
        "Verified the fix.",
        "--pr",
        pr,
    ]);
    assert_eq!(report["outcome"]["pull_requests"][0], pr);
    let listed = f.json(&["session", "list", "--run", &run]);
    assert_eq!(listed[0]["outcome"]["kind"], "done");
    assert!(f.record("true").status.success());
    let root = rhythm::private_storage::Root::open(f.path("state")).unwrap();
    let latest = rhythm::sessions::Store::existing(&root)
        .unwrap()
        .unwrap()
        .latest_for_job(JOB)
        .unwrap()
        .unwrap();
    assert_eq!(latest.id.to_string(), run);
    assert_eq!(latest.count, 2);
}

#[test]
fn claude_starts_from_the_requested_commit() {
    let f = Fixture::new();
    fn git(dir: &Path, args: &[&str]) {
        assert!(
            Command::new("git")
                .current_dir(dir)
                .args(args)
                .output()
                .unwrap()
                .status
                .success()
        );
    }
    git(f.tmp.path(), &["init", "-q"]);
    fs::write(f.path("at-base"), "base").unwrap();
    git(f.tmp.path(), &["add", "at-base"]);
    git(
        f.tmp.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "Initial fixture",
        ],
    );
    git(f.tmp.path(), &["branch", "fixture-base"]);
    fs::remove_file(f.path("at-base")).unwrap();
    git(f.tmp.path(), &["add", "-u"]);
    git(
        f.tmp.path(),
        &[
            "-c",
            "user.name=Test",
            "-c",
            "user.email=test@example.com",
            "commit",
            "-qm",
            "Advance fixture",
        ],
    );
    let launcher = f.launcher(
        r#"
if [ "$1" = agents ]; then
    [ "$3" = --cwd ] && [ -f "$4/at-base" ]
    printf '[{"id":"1234abcd","status":"blocked"}]\n'
    exit
fi
[ "$1" = --bg ] && [ "$2" = --name ] && [ "$5" = auto ]
[ -f at-base ]
[ "$7" = --add-dir ] && [ -d "$8" ]
case "$6" in *'Record the outcome in Rhythm'*) ;; *) exit 3 ;; esac
printf 'backgrounded · 1234abcd\n' >&2
"#,
    );
    f.configure("claude", &launcher, &[]);
    let result = parse(f.record(r#"
printf 'Check the fixture.' |
    "$1" --json session spawn --key test --title 'Fixture' --repo "$2" --base fixture-base --add-dir "$2"
"#));
    assert_eq!(result["launch"]["provider_id"], "1234abcd");
    let shown = f.json(&["session", "show", result["id"].as_str().unwrap()]);
    assert_eq!(shown["activity"], "waiting");
    assert!(
        result["launch"]["branch"]
            .as_str()
            .unwrap()
            .starts_with("rhythm/session-")
    );
}

#[test]
fn an_unconfirmed_launch_does_not_repeat_on_retry() {
    let f = Fixture::new();
    let launcher = f.launcher("echo attempted >> \"$0.calls\"; echo 'reply lost'");
    f.configure("t3", &launcher, &[]);
    let job = r#"
printf 'Check fixture.' | "$1" --json session spawn --key same --title 'Fixture' --repo "$2"
"#;
    for _ in 0..2 {
        assert!(!f.record(job).status.success());
    }
    assert_eq!(
        fs::read_to_string(f.path("launcher.calls")).unwrap(),
        "attempted\n"
    );
}

#[test]
fn a_definite_t3_rejection_allows_the_same_key_on_the_next_run() {
    let f = Fixture::new();
    let launcher = f.launcher(
        r#"
if [ ! -f "$0.ready" ]; then
    echo 'No T3 Code project matches fixture.' >&2
    exit 1
fi
printf '{"threadId":"synthetic-id"}\n'
"#,
    );
    f.configure("t3", &launcher, &[]);
    let job = r#"
printf 'Check fixture.' | "$1" --json session spawn --key same --title 'Fixture' --repo "$2"
"#;
    assert!(!f.record(job).status.success());
    fs::write(f.path("launcher.ready"), "").unwrap();
    assert_eq!(
        parse(f.record(job))["launch"]["provider_id"],
        "synthetic-id"
    );
}
