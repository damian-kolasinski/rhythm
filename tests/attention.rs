#![cfg(unix)]
use rhythm::{
    attention::{self, AttentionHost, AttentionOutcome, AttentionResponse, Consumer, Item},
    attention_render::render,
    attention_store::AttentionStore,
    jobs::{Health, Label, RunStore},
    launchd::Launchd,
    ledger::{self, Ledger},
    operation_error::Error,
    private_storage::Root,
    runs::{Outcome, Timestamp},
};
use std::{fs, path::Path};
use tempfile::TempDir;

mod command_fixture;

const LAUNCHCTL: &str = r#"#!/bin/sh
cd "$(dirname "$0")"
[ "$1" = print ] || exit 64
file="observed/$(printf '%s' "$2" | tr / _)"
[ -f "$file" ] || exit 113
cat "$file"
"#;
const NOW: Timestamp = Timestamp(1_790_000_000_000);

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
    fn state(&self) -> Root {
        Root::open(self.path().join("state")).unwrap()
    }
    /// A job recorded into this fixture's state directory.
    fn define(&self, label: &str) {
        let state = self.path().join("state");
        fs::write(
            self.path().join("agents").join(format!("{label}.plist")),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>/opt/bin/rhythm</string><string>--data-dir</string><string>{}</string><string>run</string><string>{label}</string><string>--</string><string>/bin/true</string></array></dict></plist>"#,
                state.display()
            ),
        )
        .unwrap();
    }
    fn observe(&self, label: &str, runs: u64, exit: i32) {
        fs::write(
            self.path()
                .join("observed")
                .join(format!("gui_501_{label}")),
            format!("\truns = {runs}\n\tlast exit code = {exit}\n"),
        )
        .unwrap();
    }
    fn record(&self, label: &str, code: i32) {
        let ledger = Ledger::open(self.state(), ledger::READER_WAIT).unwrap();
        let label: Label = label.parse().unwrap();
        let id = ledger.start(&label, 1, Timestamp(NOW.0 - 60_000)).unwrap();
        ledger
            .finish(id, Timestamp(NOW.0 - 50_000), Outcome::Code { code })
            .unwrap();
    }
    fn launchd(&self) -> Launchd {
        Launchd::at_boundary(
            self.path().join("agents"),
            self.path().join("launchctl"),
            501,
        )
    }
    /// What `rhythm attention --since-last-seen --as CONSUMER` does locally.
    fn look(&self, consumer: &str) -> Vec<Item> {
        let consumer = Consumer::parse(consumer).unwrap();
        let store = AttentionStore::open(&self.state()).unwrap();
        let previous = store.load(&consumer).unwrap();
        let (items, seen) = attention::look(&self.launchd(), &ledgers, Some(&previous)).unwrap();
        store.save(&consumer, &seen.unwrap()).unwrap();
        items
    }
}

fn ledgers(store: &RunStore) -> Option<Ledger> {
    match store {
        RunStore::Directory(dir) => {
            Ledger::open_existing(Root::open(dir.clone()).unwrap()).unwrap()
        }
        RunStore::Default => None,
    }
}

fn labels(items: &[Item]) -> Vec<(&str, Health)> {
    items.iter().map(|i| (i.label.as_str(), i.health)).collect()
}

#[test]
fn each_consumer_hears_about_a_failure_once_until_it_fails_again() {
    let f = Fixture::new();
    f.define("dev.rhythm.report");
    f.define("dev.rhythm.backup");
    f.observe("dev.rhythm.report", 1, 1);
    f.observe("dev.rhythm.backup", 1, 0);
    f.record("dev.rhythm.report", 1);
    f.record("dev.rhythm.backup", 0);

    let failing = [("dev.rhythm.report", Health::Failing)];
    assert_eq!(labels(&f.look("claude-code")), failing);
    assert!(f.look("claude-code").is_empty());
    // Another tool keeps its own place.
    assert_eq!(labels(&f.look("codex")), failing);

    f.observe("dev.rhythm.report", 2, 1);
    f.record("dev.rhythm.report", 1);
    assert_eq!(labels(&f.look("claude-code")), failing);
}

#[test]
fn failures_that_recovered_between_looks_are_still_reported() {
    let f = Fixture::new();
    f.define("dev.rhythm.sync");
    f.observe("dev.rhythm.sync", 1, 0);
    f.record("dev.rhythm.sync", 0);
    assert!(f.look("agent").is_empty());

    f.record("dev.rhythm.sync", 3);
    f.record("dev.rhythm.sync", 0);
    f.observe("dev.rhythm.sync", 3, 0);
    let items = f.look("agent");
    assert_eq!(labels(&items), [("dev.rhythm.sync", Health::Ok)]);
    assert_eq!(items[0].recovered_from.as_ref().unwrap().count, 1);
    assert!(f.look("agent").is_empty());
}

#[test]
fn without_a_previous_look_everything_needing_attention_is_reported() {
    let f = Fixture::new();
    f.define("dev.rhythm.report");
    f.observe("dev.rhythm.report", 1, 1);
    let (items, seen) = attention::look(&f.launchd(), &ledgers, None).unwrap();
    assert_eq!(labels(&items), [("dev.rhythm.report", Health::Failing)]);
    assert_eq!(seen, None);
}

#[test]
fn a_damaged_snapshot_reports_everything_once_more() {
    let f = Fixture::new();
    f.define("dev.rhythm.report");
    f.observe("dev.rhythm.report", 1, 1);
    assert_eq!(f.look("agent").len(), 1);
    fs::write(f.path().join("state/attention/agent.json"), "{broken").unwrap();
    assert_eq!(f.look("agent").len(), 1);
    assert!(Consumer::parse("../escape").is_err());
}

fn golden(name: &str, actual: &str) {
    let path = Path::new(env!("CARGO_MANIFEST_DIR")).join(format!("tests/golden/{name}.txt"));
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        fs::write(&path, actual).unwrap();
    }
    assert_eq!(actual, fs::read_to_string(&path).unwrap(), "{name}");
}

#[test]
fn renders_attention_as_actionable_lines_and_silence_as_nothing() {
    let f = Fixture::new();
    f.define("dev.rhythm.report");
    f.define("com.example.scan");
    f.define("dev.rhythm.sync");
    f.observe("dev.rhythm.report", 1, 1);
    f.record("dev.rhythm.report", 1);
    f.observe("com.example.scan", 1, 0);
    f.record("com.example.scan", 0);
    f.observe("dev.rhythm.sync", 1, 0);
    f.record("dev.rhythm.sync", 0);
    f.look("agent");
    f.record("dev.rhythm.sync", 2);
    f.record("dev.rhythm.sync", 0);
    f.observe("dev.rhythm.sync", 3, 0);
    f.observe("com.example.scan", 2, 7);
    let local = f.look("agent");
    // An unrecorded failing job, as on another machine.
    let unrecorded = Item {
        label: "com.example.watch".into(),
        health: Health::Failing,
        last_exit: Some(rhythm::jobs::Exit::Signal { signal: 9 }),
        last_run: None,
        recovered_from: None,
    };
    let response = AttentionResponse {
        hosts: vec![
            AttentionHost {
                host: "local".into(),
                outcome: AttentionOutcome::Items { items: local },
            },
            AttentionHost {
                host: "devbox".into(),
                outcome: AttentionOutcome::Items {
                    items: vec![unrecorded],
                },
            },
            AttentionHost {
                host: "laptop".into(),
                outcome: AttentionOutcome::Failed {
                    error: Error::RemoteUnavailable,
                },
            },
        ],
    };
    golden("attention", &render(&response, NOW));
    let quiet = AttentionResponse {
        hosts: vec![AttentionHost {
            host: "local".into(),
            outcome: AttentionOutcome::Items { items: vec![] },
        }],
    };
    assert_eq!(render(&quiet, NOW), "");
}

#[test]
fn the_session_hook_reports_news_once_per_agent() {
    use rhythm::{agent_setup::Agent, session_start};
    let f = Fixture::new();
    f.define("dev.rhythm.report");
    f.observe("dev.rhythm.report", 1, 1);
    f.record("dev.rhythm.report", 1);
    let store = AttentionStore::open(&f.state()).unwrap();
    let start = |agent| {
        let context = session_start::context(agent, &f.launchd(), &ledgers, &store, NOW).unwrap();
        store
            .save(&Consumer::parse(Agent::id(agent)).unwrap(), &context.seen)
            .unwrap();
        context.text
    };
    let text = start(Agent::Claude);
    assert!(
        text.starts_with("Background jobs need attention:\n! dev.rhythm.report failed: exit 1")
    );
    assert!(text.contains("`rhythm skill show`"));
    assert_eq!(start(Agent::Claude), "");
    // Codex keeps its own place.
    assert!(!start(Agent::Codex).is_empty());
}
