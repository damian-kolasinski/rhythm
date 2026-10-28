#![cfg(unix)]
use rhythm::{launchd_runtime::LaunchdRuntime, private_storage::Root, services::*};
use std::{fs, os::unix::fs::PermissionsExt, path::Path};
use tempfile::TempDir;

mod command_fixture;

struct Fixture {
    temp: TempDir,
    root: Root,
    adapter: LaunchdRuntime,
}
impl Fixture {
    fn new() -> Self {
        let temp = TempDir::new().unwrap();
        let root = Root::open(temp.path().join("state")).unwrap();
        let command = temp.path().join("launchctl");
        command_fixture::write(
            &command,
            r#"#!/bin/sh
set -eu
cd "$(dirname "$0")"
printf '%s\n' "$1" >> calls
case "$1" in
  print)
    if [ "$2" != sample-domain ] && [ -f remove-on-probe ]; then rm "$0"; fi
    [ "$2" = sample-domain ] || [ -f loaded ] ;;
  bootout) rm loaded ;;
  bootstrap) if [ -f fail-next ]; then rm fail-next; exit 1; fi; touch loaded ;;
  *) exit 1 ;;
esac
"#,
        );
        let adapter = LaunchdRuntime::at_boundary(
            temp.path().join("agents"),
            command,
            "sample-domain".into(),
        );
        Self {
            temp,
            root,
            adapter,
        }
    }
    fn registration(&self) -> std::path::PathBuf {
        fs::read_dir(self.temp.path().join("agents"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .find(|p| p.extension().is_some_and(|e| e == "plist"))
            .unwrap()
    }
    fn calls(&self, operation: &str) -> usize {
        fs::read_to_string(self.temp.path().join("calls"))
            .unwrap()
            .lines()
            .filter(|v| *v == operation)
            .count()
    }
}

#[test]
fn owned_registration_is_private_idempotent_and_removable() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    let registration = f.registration();
    assert_eq!(
        fs::metadata(&registration).unwrap().permissions().mode() & 0o777,
        0o600
    );
    let value = plist::Value::from_file(&registration).unwrap();
    let d = value.as_dictionary().unwrap();
    let args = d["ProgramArguments"].as_array().unwrap();
    assert_eq!(args[0].as_string(), Some("/sample/bin/rhythm"));
    assert_eq!(args[2].as_string(), f.root.path().to_str());
    assert_eq!(d["KeepAlive"].as_boolean(), Some(true));
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    assert_eq!(f.calls("bootstrap"), 1);
    assert_eq!(f.calls("bootout"), 0);
    assert!(f.adapter.uninstall(f.root.path()).unwrap());
    assert!(!registration.exists());
    assert!(!f.adapter.uninstall(f.root.path()).unwrap());
    assert!(f.root.path().exists());
}

#[test]
fn failed_update_restores_previous_registration_and_supervision() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    let registration = f.registration();
    let previous = fs::read(&registration).unwrap();
    fs::write(f.temp.path().join("fail-next"), "").unwrap();
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/new/rhythm"), f.root.path()),
        Err(Error::ExecutionUnavailable)
    ));
    assert_eq!(fs::read(registration).unwrap(), previous);
    assert!(f.temp.path().join("loaded").exists());
    assert_eq!(f.calls("bootstrap"), 3);
}

#[test]
fn refuses_unowned_or_unreadable_registration_and_loaded_collision() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    let registration = f.registration();
    fs::write(&registration, "synthetic unrelated registration").unwrap();
    assert!(matches!(
        f.adapter.uninstall(f.root.path()),
        Err(Error::InsecureStorage)
    ));
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/new/rhythm"), f.root.path()),
        Err(Error::InsecureStorage)
    ));
    assert_eq!(f.calls("bootout"), 0);
    fs::remove_file(&registration).unwrap();
    assert!(matches!(
        f.adapter.uninstall(f.root.path()),
        Err(Error::InsecureStorage)
    ));
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/new/rhythm"), f.root.path()),
        Err(Error::InsecureStorage)
    ));
    assert_eq!(f.calls("bootout"), 0);
}

#[test]
fn launcher_unavailable_does_not_remove_registration_or_report_absence() {
    for missing_registration in [false, true] {
        let f = Fixture::new();
        f.adapter
            .install(Path::new("/sample/bin/rhythm"), f.root.path())
            .unwrap();
        let registration = f.registration();
        let original = fs::read(&registration).unwrap();
        if missing_registration {
            fs::remove_file(&registration).unwrap();
        }
        fs::remove_file(f.temp.path().join("launchctl")).unwrap();
        assert!(matches!(
            f.adapter.uninstall(f.root.path()),
            Err(Error::ExecutionUnavailable)
        ));
        if !missing_registration {
            assert_eq!(fs::read(registration).unwrap(), original);
        }
        assert!(f.temp.path().join("loaded").exists());
    }
}

#[test]
fn unavailable_launcher_during_update_restores_previous_registration() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    let registration = f.registration();
    let original = fs::read(&registration).unwrap();
    fs::write(f.temp.path().join("remove-on-probe"), "").unwrap();
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/new/rhythm"), f.root.path()),
        Err(Error::ExecutionUnavailable)
    ));
    assert_eq!(fs::read(registration).unwrap(), original);
    assert!(f.temp.path().join("loaded").exists());
}
