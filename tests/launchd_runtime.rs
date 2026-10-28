#![cfg(unix)]
use rhythm::{
    launchd_runtime::LaunchdRuntime, operation_error::Error, private_storage::Root,
    runtime_supervision::RuntimeSupervisor,
};
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
printf '%s\n' "$*" >> calls
case "$1" in
  print)
    case "$2" in
      user/123)
        if [ -f remove-on-probe ]; then rm "$0"; fi
        [ ! -f no-user ] ;;
      gui/123) [ ! -f no-gui ] ;;
      user/123/*) [ ! -f no-user ] && [ -f loaded-user ] ;;
      gui/123/*) [ ! -f no-gui ] && [ -f loaded-gui ] ;;
      *) exit 1 ;;
    esac ;;
  bootout)
    case "$2" in
      user/123/*) rm loaded-user ;;
      gui/123/*) rm loaded-gui ;;
      *) exit 1 ;;
    esac ;;
  bootstrap)
    if [ -f fail-next ]; then rm fail-next; exit 1; fi
    case "$2" in
      user/123) [ ! -f no-user ]; touch loaded-user ;;
      gui/123) [ ! -f no-gui ]; touch loaded-gui ;;
      *) exit 1 ;;
    esac ;;
  *) exit 1 ;;
esac
"#,
        );
        let adapter = LaunchdRuntime::at_boundary(temp.path().join("agents"), command, 123);
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
            .filter(|v| v.split_whitespace().next() == Some(operation))
            .count()
    }
    fn legacy_gui_registration(&self) {
        fs::rename(
            self.temp.path().join("loaded-user"),
            self.temp.path().join("loaded-gui"),
        )
        .unwrap();
        let registration = self.registration();
        let mut value = plist::Value::from_file(&registration).unwrap();
        value
            .as_dictionary_mut()
            .unwrap()
            .remove("LimitLoadToSessionType");
        value.to_file_xml(registration).unwrap();
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
    assert_eq!(d["LimitLoadToSessionType"].as_string(), Some("Background"));
    assert!(f.temp.path().join("loaded-user").exists());
    assert!(!f.temp.path().join("loaded-gui").exists());
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
    assert!(f.temp.path().join("loaded-user").exists());
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
        assert!(f.temp.path().join("loaded-user").exists());
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
    assert!(f.temp.path().join("loaded-user").exists());
}

#[test]
fn headless_install_remains_in_the_user_domain_when_a_gui_session_appears() {
    let f = Fixture::new();
    fs::write(f.temp.path().join("no-gui"), "").unwrap();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    fs::remove_file(f.temp.path().join("no-gui")).unwrap();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    assert_eq!(f.calls("bootstrap"), 1);
    assert!(f.adapter.uninstall(f.root.path()).unwrap());
    assert!(!f.temp.path().join("loaded-user").exists());
    assert!(!f.temp.path().join("loaded-gui").exists());
}

#[test]
fn an_existing_gui_registration_is_updated_and_removed_in_its_original_domain() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    f.legacy_gui_registration();
    f.adapter
        .install(Path::new("/sample/new/rhythm"), f.root.path())
        .unwrap();
    let value = plist::Value::from_file(f.registration()).unwrap();
    assert_eq!(
        value.as_dictionary().unwrap()["LimitLoadToSessionType"].as_string(),
        Some("Aqua")
    );
    assert!(!f.temp.path().join("loaded-user").exists());
    assert!(f.temp.path().join("loaded-gui").exists());
    assert!(f.adapter.uninstall(f.root.path()).unwrap());
    assert!(!f.temp.path().join("loaded-gui").exists());
}

#[test]
fn failed_gui_update_restores_the_original_registration_in_its_domain() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    f.legacy_gui_registration();
    let previous = fs::read(f.registration()).unwrap();
    fs::write(f.temp.path().join("fail-next"), "").unwrap();
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/new/rhythm"), f.root.path()),
        Err(Error::ExecutionUnavailable)
    ));
    assert_eq!(fs::read(f.registration()).unwrap(), previous);
    assert!(f.temp.path().join("loaded-gui").exists());
    assert!(!f.temp.path().join("loaded-user").exists());
}

#[test]
fn failed_install_preserves_an_unloaded_registration_without_starting_it() {
    let f = Fixture::new();
    f.adapter
        .install(Path::new("/sample/bin/rhythm"), f.root.path())
        .unwrap();
    fs::remove_file(f.temp.path().join("loaded-user")).unwrap();
    let previous = fs::read(f.registration()).unwrap();
    fs::write(f.temp.path().join("fail-next"), "").unwrap();
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/new/rhythm"), f.root.path()),
        Err(Error::ExecutionUnavailable)
    ));
    assert_eq!(fs::read(f.registration()).unwrap(), previous);
    assert_eq!(f.calls("bootstrap"), 2);
    assert!(!f.temp.path().join("loaded-user").exists());
    assert!(!f.temp.path().join("loaded-gui").exists());
}

#[test]
fn refuses_collisions_in_either_domain_and_ambiguous_duplicate_registrations() {
    for owned_file in [false, true] {
        let f = Fixture::new();
        f.adapter
            .install(Path::new("/sample/bin/rhythm"), f.root.path())
            .unwrap();
        fs::write(f.temp.path().join("loaded-gui"), "").unwrap();
        let registration = f.registration();
        if !owned_file {
            fs::remove_file(&registration).unwrap();
            fs::remove_file(f.temp.path().join("loaded-user")).unwrap();
        }
        for result in [
            f.adapter
                .install(Path::new("/sample/new/rhythm"), f.root.path()),
            f.adapter.uninstall(f.root.path()).map(|_| ()),
        ] {
            assert!(matches!(result, Err(Error::InsecureStorage)));
        }
        assert_eq!(f.calls("bootout"), 0);
        assert!(f.temp.path().join("loaded-gui").exists());
        assert_eq!(registration.exists(), owned_file);
    }
}

#[test]
fn unavailable_user_domain_fails_without_installing_into_a_gui_session() {
    let f = Fixture::new();
    fs::write(f.temp.path().join("no-user"), "").unwrap();
    assert!(matches!(
        f.adapter
            .install(Path::new("/sample/bin/rhythm"), f.root.path()),
        Err(Error::ExecutionUnavailable)
    ));
    assert_eq!(f.calls("bootstrap"), 0);
    assert!(!f.temp.path().join("agents").exists());
}
