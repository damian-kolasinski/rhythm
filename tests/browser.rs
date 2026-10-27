use rhythm::{browser_launch, local_api, private_storage::Root, runtime_protocol::Request};
use serde_json::{Value, json};
use std::{
    process::{Child, Command, Stdio},
    time::Duration,
};
use tempfile::TempDir;
struct Fixture {
    _temp: TempDir,
    root: Root,
    runtime: Child,
    client: reqwest::Client,
    url: String,
}
impl Fixture {
    async fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = Root::open(temp.path().join("state")).unwrap();
        let runtime = Command::new(env!("CARGO_BIN_EXE_rhythm"))
            .args([
                "--data-dir",
                root.path().to_str().unwrap(),
                "runtime",
                "run",
            ])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        for _ in 0..250 {
            if local_api::call(&root, &Request::Status).await.is_ok() {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
        let launch = browser_launch::prepare(&root).await.unwrap();
        Self {
            _temp: temp,
            root,
            runtime,
            client: reqwest::Client::builder().no_proxy().build().unwrap(),
            url: launch.url.trim_end_matches('/').into(),
        }
    }
    async fn code(&self) -> String {
        browser_launch::prepare(&self.root).await.unwrap().code
    }
    async fn pair(&self, code: &str) -> String {
        let response = self
            .client
            .post(format!("{}/v1/browser/session", self.url))
            .header("Origin", &self.url)
            .json(&json!({"code":code}))
            .send()
            .await
            .unwrap();
        assert!(response.status().is_success());
        response.json::<Value>().await.unwrap()["token"]
            .as_str()
            .unwrap()
            .into()
    }
    async fn command(&self, token: &str, body: Value) -> reqwest::Response {
        self.client
            .post(format!("{}/v1/browser/command", self.url))
            .header("Origin", &self.url)
            .bearer_auth(token)
            .json(&body)
            .send()
            .await
            .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = self.runtime.kill();
        let _ = self.runtime.wait();
    }
}

#[tokio::test]
async fn pairing_requires_exact_origin_and_is_single_use_with_separate_credentials() {
    let f = Fixture::new().await;
    let code = f.code().await;
    for origin in [None, Some("http://example.invalid"), Some("null")] {
        let mut request = f
            .client
            .post(format!("{}/v1/browser/session", f.url))
            .json(&json!({"code":code}));
        if let Some(origin) = origin {
            request = request.header("Origin", origin)
        }
        assert_eq!(request.send().await.unwrap().status(), 401);
    }
    let token = f.pair(&code).await;
    assert_eq!(
        f.client
            .post(format!("{}/v1/browser/session", f.url))
            .header("Origin", &f.url)
            .json(&json!({"code":code}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    let cli = rhythm::local_access::Access::read(&f.root).unwrap();
    for credential in [cli.credential(), &code] {
        assert_eq!(
            f.command(credential, json!({"operation":"list"}))
                .await
                .status(),
            401
        );
    }
    assert_eq!(
        f.client
            .post(format!("{}/v1/command", f.url))
            .bearer_auth(&token)
            .json(&json!({"operation":"list"}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        f.client
            .post(format!("{}/v1/browser/command", f.url))
            .bearer_auth(&token)
            .json(&json!({"operation":"list"}))
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        f.command(&token, json!({"operation":"shutdown"}))
            .await
            .status(),
        403
    );
    assert!(local_api::call(&f.root, &Request::Status).await.is_ok());
    assert!(
        f.command(&token, json!({"operation":"list"}))
            .await
            .status()
            .is_success()
    );
    let response = f
        .client
        .post(format!("{}/v1/browser/disconnect", f.url))
        .header("Origin", &f.url)
        .bearer_auth(&token)
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        f.command(&token, json!({"operation":"list"}))
            .await
            .status(),
        401
    );
}

#[tokio::test]
async fn embedded_assets_and_errors_enforce_response_policy_without_private_data() {
    let f = Fixture::new().await;
    for path in ["/", "/assets/app.js", "/assets/app.css"] {
        let response = f
            .client
            .get(format!("{}{path}", f.url))
            .send()
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        assert_eq!(response.headers()["cache-control"], "no-store");
        assert_eq!(response.headers()["x-content-type-options"], "nosniff");
        assert_eq!(response.headers()["referrer-policy"], "no-referrer");
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
        assert!(
            !response
                .headers()
                .contains_key("access-control-allow-origin")
        );
        assert!(!response.headers().contains_key("set-cookie"));
    }
    assert_eq!(
        f.client
            .get(&f.url)
            .header("Host", "example.invalid")
            .send()
            .await
            .unwrap()
            .status(),
        401
    );
    assert_eq!(
        f.client
            .get(format!("{}/unlisted-file", f.url))
            .send()
            .await
            .unwrap()
            .status(),
        404
    );
    let token = f.pair(&f.code().await).await;
    let response = f
        .command(
            &token,
            json!({"operation":"create","definition":{"private":"private-source-value"}}),
        )
        .await;
    assert_eq!(response.status(), 400);
    assert!(
        !response
            .text()
            .await
            .unwrap()
            .contains("private-source-value")
    );
}

#[tokio::test]
async fn browser_commands_use_the_same_service_schedule_and_execution_state() {
    let f = Fixture::new().await;
    let token = f.pair(&f.code().await).await;
    let definition = json!({"id":"sample-browser","command":["/bin/sh","-c","printf browser-output"],"working_directory":"/tmp","restart":"never"});
    assert!(
        f.command(
            &token,
            json!({"operation":"create","definition":definition})
        )
        .await
        .status()
        .is_success()
    );
    assert!(
        f.command(&token, json!({"operation":"start","id":"sample-browser"}))
            .await
            .status()
            .is_success()
    );
    let mut execution_id = None;
    for _ in 0..150 {
        let response = f
            .command(&token, json!({"operation":"history","id":"sample-browser"}))
            .await
            .json::<Value>()
            .await
            .unwrap();
        if response["executions"][0]["outcome"]["type"] == "exited" {
            execution_id = response["executions"][0]["id"].as_i64();
            break;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    let logs = f
        .command(
            &token,
            json!({"operation":"logs","execution_id":execution_id.unwrap()}),
        )
        .await
        .json::<Value>()
        .await
        .unwrap();
    assert_eq!(logs["stdout"], "browser-output");
    let definition = json!({"id":"sample-browser","command":["/usr/bin/true"],"working_directory":"/tmp","trigger":{"type":"interval","every_ms":1000},"overlap":"skip","missed":"skip"});
    assert!(
        f.command(
            &token,
            json!({"operation":"schedule","command":{"operation":"create","definition":definition}})
        )
        .await
        .status()
        .is_success()
    );
    let services = local_api::call(&f.root, &rhythm::services::Request::List)
        .await
        .unwrap();
    assert!(matches!(services,rhythm::services::Response::Services{services} if services.len()==1));
    let schedules = local_api::call(
        &f.root,
        &Request::Schedule {
            command: rhythm::schedules::Request::List,
        },
    )
    .await
    .unwrap();
    assert!(
        matches!(schedules,rhythm::runtime_protocol::Response::Schedule{response:rhythm::schedules::Response::Schedules{schedules}} if schedules.len()==1)
    );
}

#[tokio::test]
async fn browser_launcher_rejects_terminal_controls_from_a_reused_port() {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let temp = TempDir::new().unwrap();
    let root = Root::open(temp.path().join("state")).unwrap();
    let listener = tokio::net::TcpListener::bind((std::net::Ipv4Addr::LOCALHOST, 0))
        .await
        .unwrap();
    rhythm::local_access::Access::create(listener.local_addr().unwrap().port())
        .unwrap()
        .save(&root)
        .unwrap();
    let server = tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.unwrap();
        let mut request = [0; 8192];
        let _ = stream.read(&mut request).await;
        let body = json!({"code":"\u{1b}[31mprivate-source-value","demo":false}).to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body
        );
        stream.write_all(response.as_bytes()).await.unwrap();
    });
    assert!(browser_launch::prepare(&root).await.is_err());
    server.await.unwrap();
}

#[test]
fn demo_delivery_failure_drains_runtime_without_waiting_for_client_timeout() {
    let mut child = Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .args(["--json", "demo", "--no-open"])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .unwrap();
    drop(child.stdout.take());
    for _ in 0..300 {
        if let Some(status) = child.try_wait().unwrap() {
            assert!(!status.success());
            return;
        }
        std::thread::sleep(Duration::from_millis(20));
    }
    let _ = child.kill();
    let _ = child.wait();
    panic!("demo did not drain after delivery failure");
}
