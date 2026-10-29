//! Opt-in checks against the real user launchd. They register disposable agents
//! with a random label and remove them afterwards:
//! `cargo test --test launchd_e2e -- --ignored`
#![cfg(target_os = "macos")]
use rhythm::{
    jobs::{Exit, Health, Observation},
    launchd::Launchd,
    status,
};
use std::{fs, path::Path, process::Command, time::Duration};
use tempfile::TempDir;

struct Agent {
    domain: String,
    label: String,
}
impl Agent {
    fn register(agents: &Path, program: impl Fn(&str) -> Vec<String>) -> Self {
        let user = unsafe { libc::getuid() };
        let mut bytes = [0u8; 6];
        getrandom::fill(&mut bytes).unwrap();
        let suffix: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let label = format!("dev.rhythm.test.{suffix}");
        let plist = agents.join(format!("{label}.plist"));
        let gui = format!("gui/{user}");
        let domain = if launchctl(&["print", &gui]) {
            gui
        } else {
            format!("user/{user}")
        };
        let session = if domain.starts_with("gui") {
            "Aqua"
        } else {
            "Background"
        };
        let arguments: String = program(&label)
            .iter()
            .map(|a| format!("<string>{a}</string>"))
            .collect();
        fs::write(
            &plist,
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array>{arguments}</array><key>RunAtLoad</key><true/><key>LimitLoadToSessionType</key><string>{session}</string></dict></plist>"#
            ),
        )
        .unwrap();
        assert!(launchctl(&["bootstrap", &domain, plist.to_str().unwrap()]));
        Self { domain, label }
    }
}
impl Drop for Agent {
    fn drop(&mut self) {
        launchctl(&["bootout", &format!("{}/{}", self.domain, self.label)]);
    }
}

fn launchctl(args: &[&str]) -> bool {
    Command::new("/bin/launchctl")
        .args(args)
        .output()
        .is_ok_and(|o| o.status.success())
}

#[test]
#[ignore = "registers a disposable agent with the real user launchd"]
fn reports_a_real_agent_failure() {
    let temp = TempDir::new().unwrap();
    let agent = Agent::register(temp.path(), |_| shell("exit 3"));
    let launchd = Launchd::at_boundary(temp.path().to_owned(), "/bin/launchctl".into(), unsafe {
        libc::getuid()
    });
    let mut last = None;
    for _ in 0..100 {
        let reports = status::jobs(&launchd, &|_| None).unwrap();
        if reports[0].health == Health::Failing {
            last = Some(reports[0].clone());
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let report = last.expect("the agent's failure was never observed");
    assert_eq!(report.label, agent.label);
    assert!(matches!(
        report.observation,
        Observation::Loaded {
            pid: None,
            runs: 1,
            last_exit: Some(Exit::Code { code: 3 })
        }
    ));
    drop(agent);
    assert_eq!(
        status::jobs(&launchd, &|_| None).unwrap()[0].health,
        Health::Unloaded
    );
}

fn shell(script: &str) -> Vec<String> {
    ["/bin/sh", "-c", script].map(str::to_owned).to_vec()
}

#[test]
#[ignore = "registers a disposable agent with the real user launchd"]
fn records_a_real_agent_run() {
    use rhythm::{
        ledger::Ledger,
        private_storage::Root,
        runs::{Outcome, RunState},
    };
    let temp = TempDir::new().unwrap();
    let state = temp.path().join("state");
    let agents = temp.path().join("agents");
    fs::create_dir(&agents).unwrap();
    let agent = Agent::register(&agents, |label| {
        let mut program = vec![
            env!("CARGO_BIN_EXE_rhythm").to_owned(),
            "--data-dir".into(),
            state.to_str().unwrap().into(),
            "run".into(),
            label.into(),
            "--".into(),
        ];
        program.extend(shell("echo recorded by launchd; exit 3"));
        program
    });
    let launchd = Launchd::at_boundary(agents, "/bin/launchctl".into(), unsafe { libc::getuid() });
    let mut report = None;
    for _ in 0..200 {
        let reports = status::jobs(&launchd, &|_| {
            Root::open(state.clone())
                .ok()
                .and_then(|root| Ledger::open_existing(root).ok().flatten())
        })
        .unwrap();
        if reports[0].health == Health::Failing && reports[0].last_run.is_some() {
            report = Some(reports[0].clone());
            break;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    let report = report.expect("the recorded failure was never observed");
    assert_eq!(report.label, agent.label);
    assert!(report.recorded);
    // launchd sees the command's own exit status through the recorder.
    assert!(matches!(
        report.observation,
        Observation::Loaded {
            last_exit: Some(Exit::Code { code: 3 }),
            ..
        }
    ));
    assert!(matches!(
        report.last_run.unwrap().state,
        RunState::Finished {
            outcome: Outcome::Code { code: 3 },
            ..
        }
    ));
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .args(["--data-dir", state.to_str().unwrap(), "logs", &agent.label])
        .output()
        .unwrap();
    assert_eq!(output.stdout, b"recorded by launchd\n");
}
