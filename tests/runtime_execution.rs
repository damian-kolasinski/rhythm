#![cfg(unix)]
use rhythm::{
    execution_records::*,
    local_access::Access,
    local_api,
    operation_error::Error,
    private_storage::{self, Root},
    runtime_protocol::Request as RuntimeRequest,
    schedules::*,
};
use serde_json::json;
use std::{
    fs,
    os::unix::fs::PermissionsExt,
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
            .env("RHYTHM_FIXTURE_SECRET", "private-fixture-value")
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
        panic!("runtime did not become ready")
    }
    async fn call(&self, request: Request) -> Response {
        local_api::call(&self.root, &request)
            .await
            .expect("fixture request succeeds")
    }

    fn id(id: &str) -> ScheduleId {
        id.parse().unwrap()
    }
    async fn create(&self, id: &str, script: &str) {
        let definition=serde_json::from_value(json!({"id":id,"command":["/bin/sh","-c",script],"working_directory":self.temp.path(),"trigger":{"type":"interval","every_ms":60000},"overlap":"skip","missed":"skip"})).unwrap();
        self.call(Request::Create { definition }).await;
    }
    async fn history(&self, id: &str) -> Vec<Execution> {
        match self.call(Request::History { id: Self::id(id) }).await {
            Response::History { executions } => executions,
            _ => panic!("history response"),
        }
    }
    async fn wait_finished(&self, id: &str, count: usize) -> Vec<Execution> {
        for _ in 0..300 {
            let e = self.history(id).await;
            if e.len() >= count && e[0].outcome.is_some() {
                return e;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("execution did not finish")
    }
    async fn wait_running(&self, id: &str) -> Execution {
        for _ in 0..300 {
            if let Response::Schedule { schedule, .. } =
                self.call(Request::Inspect { id: Self::id(id) }).await
                && matches!(schedule.observed, Observed::Running { .. })
            {
                return schedule.latest_execution.unwrap();
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        panic!("schedule did not run")
    }
    fn cli(&self, args: &[&str]) -> serde_json::Value {
        let o = Command::new(env!("CARGO_BIN_EXE_rhythm"))
            .args(["--json", "--data-dir", self.root.path().to_str().unwrap()])
            .args(args)
            .output()
            .unwrap();
        assert!(o.status.success(), "CLI request failed");
        serde_json::from_slice(&o.stdout).unwrap()
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
async fn wait_child(child: &mut Child) {
    for _ in 0..250 {
        if child.try_wait().unwrap().is_some() {
            return;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    panic!("runtime did not stop")
}
async fn wait_file(path: PathBuf) -> String {
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

#[test]
fn cli_exposes_only_schedules_and_runtime_controls() {
    let help = Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .arg("--help")
        .output()
        .unwrap();
    assert!(help.status.success());
    let help = String::from_utf8(help.stdout).unwrap();
    assert!(help.contains("schedule"));
    assert!(!help.contains("service"));
    assert!(!help.contains("discover"));
    for command in ["service", "discover"] {
        let output = Command::new(env!("CARGO_BIN_EXE_rhythm"))
            .args(["--json", command])
            .output()
            .unwrap();
        assert!(!output.status.success());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&output.stderr).unwrap()["error"]["code"],
            "invalid_request"
        );
    }
}

#[tokio::test]
async fn finite_schedule_cli_history_logs_and_environment_isolation() {
    let f = Fixture::new().await;
    f.create("sample-finite","[ -z \"$RHYTHM_FIXTURE_SECRET\" ] || exit 91; printf sample-output; printf sample-error >&2; exit 7",).await;
    f.cli(&["schedule", "run-now", "sample-finite"]);
    let e = f.wait_finished("sample-finite", 1).await;
    assert_eq!(e[0].outcome, Some(Outcome::Exited { code: 7 }));
    let logs = f.cli(&["schedule", "logs", &e[0].id.to_string()]);
    assert_eq!(logs["stdout"], "sample-output");
    assert_eq!(logs["stderr"], "sample-error");
    assert_eq!(
        f.cli(&["schedule", "list"])["schedules"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    let duplicate=serde_json::from_value(json!({"id":"sample-finite","command":["/bin/true"],"working_directory":f.temp.path(),"trigger":{"type":"interval","every_ms":60000},"overlap":"skip","missed":"skip"})).unwrap();
    assert!(matches!(
        local_api::call(
            &f.root,
            &Request::Create {
                definition: duplicate
            }
        )
        .await,
        Err(Error::AlreadyExists)
    ));
}

#[tokio::test]
async fn normal_exit_cleans_redirected_descendants() {
    let f = Fixture::new().await;
    f.create(
        "sample-parent",
        "sleep 30 </dev/null >/dev/null 2>&1 & echo $! > child.pid; exit 0",
    )
    .await;
    f.call(Request::RunNow {
        id: Fixture::id("sample-parent"),
    })
    .await;
    let pid = wait_file(f.temp.path().join("child.pid"))
        .await
        .trim()
        .parse()
        .unwrap();
    assert_eq!(
        f.wait_finished("sample-parent", 1).await[0].outcome,
        Some(Outcome::Exited { code: 0 })
    );
    assert!(!process_running(pid));
}

#[tokio::test]
async fn singleton_and_api_authentication_reject_untrusted_requests() {
    let f = Fixture::new().await;
    let mut second = Fixture::spawn(&f.root);
    wait_child(&mut second).await;
    assert!(!second.wait().unwrap().success());
    let access = Access::read(&f.root).unwrap();
    let url = format!("http://127.0.0.1:{}/v1/command", access.port);
    let client = reqwest::Client::builder().no_proxy().build().unwrap();
    for (auth, origin, host) in [
        (None, None, None),
        (Some("wrong"), None, None),
        (
            Some(access.credential()),
            Some("http://example.invalid"),
            None,
        ),
        (Some(access.credential()), None, Some("example.invalid")),
    ] {
        let mut req = client.post(&url).json(&json!({"operation":"status"}));
        if let Some(v) = auth {
            req = req.bearer_auth(v)
        }
        if let Some(v) = origin {
            req = req.header("Origin", v)
        }
        if let Some(v) = host {
            req = req.header("Host", v)
        }
        assert_eq!(req.send().await.unwrap().status(), 401);
    }
    let bad = client
        .post(&url)
        .bearer_auth(access.credential())
        .json(&json!({"operation":"create","definition":{"private":"private-source-value"}}))
        .send()
        .await
        .unwrap();
    assert_eq!(bad.status(), 400);
    assert!(!bad.text().await.unwrap().contains("private-source-value"));
    assert!(matches!(
        local_api::call(
            &f.root,
            &Request::Inspect {
                id: Fixture::id("missing")
            }
        )
        .await,
        Err(Error::NotFound)
    ));
}

#[tokio::test]
async fn shutdown_with_concurrent_requests_finishes() {
    let mut f = Fixture::new().await;
    let mut requests = vec![];
    for _ in 0..50 {
        let root = f.root.clone();
        requests.push(tokio::spawn(async move {
            local_api::call(&root, &RuntimeRequest::Status).await
        }));
    }
    local_api::call(&f.root, &rhythm::runtime_protocol::Request::Shutdown)
        .await
        .unwrap();
    wait_child(&mut f.runtime).await;
    for request in requests {
        let _ = tokio::time::timeout(Duration::from_secs(3), request)
            .await
            .expect("handler did not finish");
    }
}

#[tokio::test]
async fn output_is_bounded_and_excess_is_drained() {
    let f = Fixture::new().await;
    f.create("sample-noisy", "head -c 3145728 /dev/zero; printf done >&2")
        .await;
    f.call(Request::RunNow {
        id: Fixture::id("sample-noisy"),
    })
    .await;
    let e = f.wait_finished("sample-noisy", 1).await;
    assert_eq!(e[0].outcome, Some(Outcome::Exited { code: 0 }));
    let dir = f.root.run(e[0].id).unwrap();
    assert_eq!(fs::metadata(dir.join("stdout.log")).unwrap().len(), 1048576);
    assert_eq!(
        fs::metadata(dir.join("stdout.log"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    assert_eq!(
        fs::metadata(f.root.path()).unwrap().permissions().mode() & 0o777,
        0o700
    );
    if let rhythm::runtime_protocol::Response::Logs { stdout, stderr } = local_api::call(
        &f.root,
        &rhythm::runtime_protocol::Request::Logs {
            execution_id: e[0].id,
        },
    )
    .await
    .unwrap()
    {
        assert_eq!(stdout.len(), 65536);
        assert_eq!(stderr, "done")
    } else {
        panic!("logs response")
    }
}

#[test]
fn storage_rejects_permissive_directories_and_symlinks() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("permissive");
    fs::create_dir(&dir).unwrap();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(Root::open(dir), Err(Error::InsecureStorage)));
    let link = temp.path().join("link");
    std::os::unix::fs::symlink(temp.path(), &link).unwrap();
    assert!(matches!(Root::open(link), Err(Error::InsecureStorage)));
    let target = temp.path().join("target");
    fs::write(&target, "fixture").unwrap();
    let filelink = temp.path().join("filelink");
    std::os::unix::fs::symlink(&target, &filelink).unwrap();
    assert!(private_storage::private_file(&filelink).is_err());
    assert_eq!(fs::read_to_string(&target).unwrap(), "fixture");
}

#[test]
fn cli_argument_errors_do_not_echo_untrusted_values() {
    for args in [
        vec!["--json", "schedule", "run-now", "../private-source-value"],
        vec!["--json", "schedule", "logs", "private-source-value"],
        vec!["--json", "private-source-value"],
    ] {
        let output = Command::new(env!("CARGO_BIN_EXE_rhythm"))
            .args(args)
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error = String::from_utf8(output.stderr).unwrap();
        assert!(!error.contains("private-source-value"));
        assert_eq!(
            serde_json::from_str::<serde_json::Value>(&error).unwrap()["error"]["code"],
            "invalid_request"
        );
    }
}

#[cfg(target_os = "macos")]
#[test]
fn storage_rejects_extended_acl_grants_and_inheritance_before_writing_secrets() {
    let temp = TempDir::new().unwrap();
    let dir = temp.path().join("state");
    Root::open(dir.clone()).unwrap();
    assert!(
        Command::new("/bin/chmod")
            .args([
                "+a",
                "everyone allow read,search,file_inherit,directory_inherit"
            ])
            .arg(&dir)
            .status()
            .unwrap()
            .success()
    );
    assert!(matches!(
        Root::open(dir.clone()),
        Err(Error::InsecureStorage)
    ));
    assert!(matches!(
        Root::open(dir.join("nested")),
        Err(Error::InsecureStorage)
    ));
    let secret = dir.join("secret.json");
    assert!(private_storage::write_json(&secret, &"synthetic-value").is_err());
    assert!(!secret.exists());
    let file = dir.join("fixture");
    fs::write(&file, "synthetic-value").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o600)).unwrap();
    assert!(matches!(
        private_storage::private_file(&file),
        Err(Error::InsecureStorage)
    ));
    assert!(private_storage::read_json::<String>(&file).is_err());
}

#[tokio::test]
async fn runtime_stop_shuts_down_foreground_owner_and_is_idempotent() {
    let mut f = Fixture::new().await;
    f.create("sample-stop", "exec sleep 30").await;
    f.call(Request::RunNow {
        id: Fixture::id("sample-stop"),
    })
    .await;
    f.wait_running("sample-stop").await;
    f.cli(&["runtime", "stop"]);
    wait_child(&mut f.runtime).await;
    assert!(f.root.lock().is_ok());
    f.cli(&["runtime", "stop"]);
}

#[cfg(target_os = "macos")]
#[tokio::test]
#[ignore = "requires a macOS user launchd domain; creates and removes one disposable registration"]
async fn launchd_install_cli_schedule_and_stop_are_verified_end_to_end() {
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
    // Installing supervision while a foreground owner holds the lock must not
    // make Stop mistake removing the registration for stopping that owner.
    f.cli(&["runtime", "install"]);
    f.cli(&["runtime", "stop"]);
    wait_child(&mut f.runtime).await;
    assert!(f.root.lock().is_ok());
    f.cli(&["runtime", "install"]);
    f.ready().await;
    f.cli(&["runtime", "install"]);
    f.create("sample-launched", "printf launched-output; exit 0")
        .await;
    f.cli(&["schedule", "run-now", "sample-launched"]);
    let records = f.wait_finished("sample-launched", 1).await;
    assert_eq!(records[0].outcome, Some(Outcome::Exited { code: 0 }));
    assert_eq!(
        f.cli(&["schedule", "logs", &records[0].id.to_string()])["stdout"],
        "launched-output"
    );
    f.cli(&["runtime", "stop"]);
    assert!(f.root.lock().is_ok());
    f.cli(&["runtime", "install"]);
    f.ready().await;
    assert_eq!(f.history("sample-launched").await.len(), 1);
    f.cli(&["runtime", "stop"]);
    assert!(f.root.path().join("state.sqlite").exists());
}
