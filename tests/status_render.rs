//! Golden tests for terminal status output. Set UPDATE_GOLDEN=1 to rewrite them.
use rhythm::{
    jobs::{Activation, Calendar, Exit, Health, Observation, Trigger},
    operation_error::Error,
    status::{HostOutcome, HostStatus, JobReport},
    status_render::render,
};
use std::path::Path;

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, actual).unwrap();
    }
    assert_eq!(actual, std::fs::read_to_string(&path).unwrap(), "{name}");
}

fn job(label: &str, activation: Option<Activation>, observation: Observation) -> JobReport {
    JobReport {
        label: label.into(),
        health: Health::assess(activation.as_ref(), &observation),
        activation,
        observation,
    }
}

fn loaded(pid: Option<u32>, last_exit: Option<Exit>) -> Observation {
    Observation::Loaded {
        pid,
        runs: 3,
        last_exit,
    }
}

fn daily(hour: u8, minute: u8) -> Activation {
    Activation::Triggered {
        triggers: vec![Trigger::Calendar(Calendar {
            hour: Some(hour),
            minute: Some(minute),
            ..Calendar::default()
        })],
    }
}

fn mixed() -> Vec<JobReport> {
    vec![
        job("com.example.broken", None, Observation::Unloaded),
        job(
            "com.example.scan",
            Some(daily(8, 7)),
            loaded(None, Some(Exit::Code { code: 1 })),
        ),
        job(
            "com.example.server",
            Some(Activation::Service { always: true }),
            loaded(None, Some(Exit::Signal { signal: 9 })),
        ),
        job(
            "com.example.sync",
            Some(Activation::Triggered {
                triggers: vec![Trigger::Interval { seconds: 300 }, Trigger::QueueDirectory],
            }),
            loaded(Some(4242), None),
        ),
        job(
            "com.example.report",
            Some(daily(18, 30)),
            loaded(None, Some(Exit::Code { code: 0 })),
        ),
        job(
            "com.example.helper",
            Some(Activation::Triggered { triggers: vec![] }),
            loaded(None, None),
        ),
        job("com.example.off", Some(daily(6, 0)), Observation::Unloaded),
    ]
}

fn local(jobs: Vec<JobReport>) -> HostStatus {
    HostStatus {
        host: "local".into(),
        outcome: HostOutcome::Jobs { jobs },
    }
}

#[test]
fn renders_mixed_local_jobs() {
    golden("status-mixed", &render(&[local(mixed())], false));
}

#[test]
fn renders_attention_in_color() {
    golden(
        "status-color",
        &render(&[local(mixed()[..4].to_vec())], true),
    );
}

#[test]
fn renders_empty_and_healthy_machines() {
    let healthy = vec![job(
        "com.example.report",
        Some(daily(9, 0)),
        loaded(None, Some(Exit::Code { code: 0 })),
    )];
    golden(
        "status-quiet",
        &(render(&[local(vec![])], false) + "---\n" + &render(&[local(healthy)], false)),
    );
}

#[test]
fn renders_hosts_as_sections_with_failures_inline() {
    golden(
        "status-hosts",
        &render(
            &[
                local(mixed()[3..5].to_vec()),
                HostStatus {
                    host: "devbox".into(),
                    outcome: HostOutcome::Jobs {
                        jobs: mixed()[1..2].to_vec(),
                    },
                },
                HostStatus {
                    host: "laptop".into(),
                    outcome: HostOutcome::Failed {
                        error: Error::RemoteUnavailable,
                    },
                },
                HostStatus {
                    host: "build".into(),
                    outcome: HostOutcome::Failed {
                        error: Error::RemoteIncompatible,
                    },
                },
            ],
            false,
        ),
    );
}

#[test]
fn renders_terminal_controls_in_labels_as_placeholders() {
    let hostile = job(
        "com.example.\x1b]52;c;evil\x07",
        Some(daily(1, 2)),
        loaded(None, Some(Exit::Code { code: 0 })),
    );
    let output = render(&[local(vec![hostile])], false);
    assert!(!output.contains('\x1b') && !output.contains('\x07'));
    assert!(output.contains("com.example.?]52;c;evil?"));
    let spoofed = job(
        "com.example.\u{202E}gpj.exe",
        Some(daily(1, 2)),
        loaded(None, Some(Exit::Code { code: 0 })),
    );
    assert!(render(&[local(vec![spoofed])], false).contains("com.example.?gpj.exe"));
}
