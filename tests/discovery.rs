#![cfg(unix)]
use rhythm::{
    discovery::{self, DiagnosticCode, ObservedState, Requirement, Trigger, WorkKind},
    launchd::LaunchdInventory,
};
use std::{fs, path::Path, process::Command};
use tempfile::TempDir;

mod command_fixture;

fn write_registration(directory: &Path, name: &str, label: &str, extra: &str, binary: bool) {
    let xml = format!(
        "<plist version=\"1.0\"><dict><key>Label</key><string>{label}</string><key>ProgramArguments</key><array><string>/bin/sh</string><string>-c</string><string>private-command-argument</string></array>{extra}</dict></plist>"
    );
    let value = plist::Value::from_reader(std::io::Cursor::new(xml)).unwrap();
    let path = directory.join(name);
    if binary {
        value.to_file_binary(path).unwrap();
    } else {
        value.to_file_xml(path).unwrap();
    }
}

fn launchctl(directory: &Path, body: &str) -> std::path::PathBuf {
    let path = directory.join("launchctl");
    command_fixture::write(
        &path,
        &format!("#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = list ] || exit 99\n{body}\n"),
    );
    path
}

#[test]
fn discovers_xml_and_binary_registrations_with_real_files_and_external_command_boundary() {
    let dir = TempDir::new().unwrap();
    write_registration(
        dir.path(),
        "service.plist",
        "service",
        "<key>KeepAlive</key><true/><key>EnvironmentVariables</key><dict><key>SECRET_NAME</key><string>private-environment-value</string></dict>",
        true,
    );
    write_registration(
        dir.path(),
        "schedule.plist",
        "schedule",
        "<key>StartInterval</key><integer>300</integer><key>RunAtLoad</key><true/>",
        false,
    );
    write_registration(dir.path(), "unloaded.plist", "unloaded", "", false);
    let before = fs::read(dir.path().join("service.plist")).unwrap();
    let executable = launchctl(
        dir.path(),
        "printf 'PID\\tStatus\\tLabel\\n33\\t-9\\tservice\\n-\\t1\\tschedule\\n'",
    );
    let inventory =
        discovery::discover(&LaunchdInventory::new(dir.path().into()).with_launchctl(executable));
    assert!(inventory.complete);
    assert_eq!(inventory.registrations.len(), 3);
    let schedule = &inventory.registrations[0];
    assert_eq!(schedule.label, "schedule");
    assert_eq!(schedule.kind, WorkKind::Schedule);
    assert_eq!(schedule.observed, ObservedState::Idle { last_exit_code: 1 });
    assert_eq!(
        schedule.triggers,
        vec![Trigger::Interval { seconds: 300 }, Trigger::AtLoad]
    );
    let service = &inventory.registrations[1];
    assert_eq!(
        service.observed,
        ObservedState::Running {
            pid: 33,
            previous_exit_code: -9
        }
    );
    assert!(service.requirements.contains(&Requirement::RestartAlways));
    assert_eq!(
        inventory.registrations[2].observed,
        ObservedState::NotObserved
    );
    let json = serde_json::to_string(&inventory).unwrap();
    for secret in [
        "private-command-argument",
        "private-environment-value",
        "SECRET_NAME",
    ] {
        assert!(!json.contains(secret));
    }
    assert_eq!(before, fs::read(dir.path().join("service.plist")).unwrap());
}

#[test]
fn malformed_registrations_and_unavailable_observation_produce_partial_inventory() {
    let dir = TempDir::new().unwrap();
    write_registration(dir.path(), "valid.plist", "valid", "", false);
    fs::write(dir.path().join("invalid.plist"), "private-invalid-source").unwrap();
    let executable = launchctl(dir.path(), "echo private-process-error >&2; exit 1");
    let inventory =
        discovery::discover(&LaunchdInventory::new(dir.path().into()).with_launchctl(executable));
    assert!(!inventory.complete);
    assert_eq!(inventory.registrations[0].observed, ObservedState::Unknown);
    assert!(
        inventory
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::InvalidRegistration)
    );
    assert!(
        inventory
            .diagnostics
            .iter()
            .any(|d| d.code == DiagnosticCode::ObservationUnavailable)
    );
    let json = serde_json::to_string(&inventory).unwrap();
    assert!(!json.contains("private-"));
}

#[test]
fn duplicate_labels_are_not_silently_collapsed() {
    let dir = TempDir::new().unwrap();
    write_registration(dir.path(), "a.plist", "duplicate", "", false);
    write_registration(dir.path(), "b.plist", "duplicate", "", false);
    let executable = launchctl(dir.path(), "printf 'PID\\tStatus\\tLabel\\n'");
    let inventory =
        discovery::discover(&LaunchdInventory::new(dir.path().into()).with_launchctl(executable));
    assert!(!inventory.complete);
    assert_eq!(inventory.registrations.len(), 2);
    assert_eq!(
        inventory.diagnostics[0].code,
        DiagnosticCode::DuplicateLabel
    );
}

#[test]
fn special_files_symlinks_and_oversized_plists_cannot_block_other_registrations() {
    let dir = TempDir::new().unwrap();
    write_registration(dir.path(), "valid.plist", "valid", "", false);
    let fifo = dir.path().join("pipe.plist");
    assert!(
        Command::new("/usr/bin/mkfifo")
            .arg(&fifo)
            .status()
            .unwrap()
            .success()
    );
    std::os::unix::fs::symlink(&fifo, dir.path().join("symlink.plist")).unwrap();
    let oversized = fs::File::create(dir.path().join("oversized.plist")).unwrap();
    oversized.set_len(1024 * 1024 + 1).unwrap();
    let executable = launchctl(dir.path(), "printf 'PID\\tStatus\\tLabel\\n'");
    let inventory =
        discovery::discover(&LaunchdInventory::new(dir.path().into()).with_launchctl(executable));
    assert!(!inventory.complete);
    assert_eq!(inventory.registrations.len(), 1);
    assert_eq!(inventory.diagnostics.len(), 3);
    assert!(
        inventory
            .diagnostics
            .iter()
            .all(|d| d.code == DiagnosticCode::UnsupportedRegistration)
    );
}

#[test]
fn missing_labels_do_not_claim_unloaded_across_bootstrap_domains() {
    let dir = TempDir::new().unwrap();
    write_registration(dir.path(), "gui.plist", "gui-only", "", false);
    let executable = launchctl(
        dir.path(),
        "printf 'PID\\tStatus\\tLabel\\n14\\t0\\tssh-only\\n'",
    );
    let inventory =
        discovery::discover(&LaunchdInventory::new(dir.path().into()).with_launchctl(executable));
    assert_eq!(inventory.observation_scope, "calling_launchd_context");
    assert_eq!(
        inventory.registrations[0].observed,
        ObservedState::NotObserved
    );
}

#[test]
fn absent_directory_is_empty_but_a_file_in_its_place_is_an_error() {
    let dir = TempDir::new().unwrap();
    let executable = launchctl(dir.path(), "printf 'PID\\tStatus\\tLabel\\n'");
    let path = dir.path().join("LaunchAgents");
    assert!(
        discovery::discover(
            &LaunchdInventory::new(path.clone()).with_launchctl(executable.clone())
        )
        .complete
    );
    fs::write(&path, "not a directory").unwrap();
    let inventory = discovery::discover(&LaunchdInventory::new(path).with_launchctl(executable));
    assert!(!inventory.complete);
    assert_eq!(
        inventory.diagnostics[0].code,
        DiagnosticCode::DirectoryUnreadable
    );
}

#[cfg(target_os = "macos")]
#[test]
fn cli_end_to_end_uses_real_launchctl_and_preserves_registrations() {
    let dir = TempDir::new().unwrap();
    let agents = dir.path().join("Library/LaunchAgents");
    fs::create_dir_all(&agents).unwrap();
    write_registration(
        &agents,
        "fixture.plist",
        "dev.rhythm.test-unloaded",
        "<key>StartInterval</key><integer>300</integer>",
        false,
    );
    let before = fs::read(agents.join("fixture.plist")).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .env("HOME", dir.path())
        .args(["discover", "--json"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["ownership"], "external_read_only");
    assert_eq!(
        value["registrations"][0]["observed"]["state"],
        "not_observed"
    );
    assert_eq!(value["registrations"][0]["triggers"][0]["seconds"], 300);
    assert_eq!(before, fs::read(agents.join("fixture.plist")).unwrap());
    fs::write(agents.join("broken.plist"), "private-source").unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .env("HOME", dir.path())
        .args(["--json", "discover"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(!String::from_utf8_lossy(&output.stdout).contains("private-source"));
    assert!(output.stderr.is_empty());
}

#[test]
fn cli_rejects_relative_home_with_predictable_json_error() {
    let output = Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .env("HOME", "relative")
        .args(["--json", "discover"])
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(
        error["error"]["code"],
        if cfg!(target_os = "macos") {
            "invalid_home"
        } else {
            "unsupported_platform"
        }
    );
}
