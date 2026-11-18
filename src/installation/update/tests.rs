use std::{cell::Cell, os::unix::fs::PermissionsExt};

use super::super::{install as place, window};
use super::*;
use crate::launchd::Launchd;

struct StubCatalog {
    response: Result<Option<Version>>,
    calls: Cell<usize>,
}

impl Catalog for StubCatalog {
    fn latest(&self) -> Result<Option<Version>> {
        self.calls.set(self.calls.get() + 1);
        self.response.clone()
    }
}

const REPOSITORY: &str = "example/rhythm";
const TOOL: &str = "packslip:github.com/example/rhythm";
const FAKE_MISE: &str = r#"#!/bin/sh
set -eu
case "$1" in
  trust)
    test "$2" = "$MISE_CONFIG_FILE"
    grep -q 'workflows/release.yml@refs/tags/v0.3.1' "$MISE_CONFIG_FILE"
    grep -q 'https://token.actions.githubusercontent.com' "$MISE_CONFIG_FILE"
    ;;
  install)
    test ! -e "$HOME/reject-release"
    mkdir -p "$MISE_DATA_DIR/verified"
    cp "$HOME/rhythm-fixture" "$MISE_DATA_DIR/verified/rhythm"
    cp "$HOME/window-fixture" "$MISE_DATA_DIR/verified/rhythm-ui"
    cp "$HOME/icon-fixture" "$MISE_DATA_DIR/verified/Rhythm.icns"
    touch "$MISE_DATA_DIR/verified/.mise-packslip.json"
    ;;
  which) printf '%s\n' "$MISE_DATA_DIR/verified/rhythm" ;;
  ls)
    printf '[{"version":"0.3.0","source":{"path":"%s/mise-test.toml"}}]\n' "$HOME"
    ;;
  where)
    test -e "$HOME/linked-release"
    printf '%s\n' "$HOME/linked-release"
    ;;
  link)
    ln -s "$3" "$HOME/linked-release"
    ;;
  uninstall)
    rm "$HOME/linked-release"
    ;;
  *) exit 64 ;;
esac
"#;

struct DesktopStub {
    config: std::path::PathBuf,
    opens: bool,
}

impl Desktop for DesktopStub {
    fn available(&self) -> bool {
        true
    }
    fn sign(&self, _: &Path) -> Result<()> {
        Ok(())
    }
    fn open(&self, _: &Path, _: &Path) -> Result<()> {
        // Exercise the real config edit; an unrelated change made while the
        // window is opening must survive both success and rollback.
        let output = fs::read_to_string(&self.config).unwrap();
        assert!(output.contains("version = \"0.3.1\""));
        fs::write(
            &self.config,
            output.replace("sample = \"1\"", "sample = \"2\""),
        )
        .unwrap();
        self.opens.then_some(()).ok_or(Error::WindowUnavailable)
    }
}

#[test]
fn verification_gates_install_and_readiness_failure_restores_only_rhythms_request() {
    let scratch = tempfile::tempdir().unwrap();
    let paths = Paths::new(scratch.path().canonicalize().unwrap()).unwrap();
    let write_executable = |path: &Path, content: &str| {
        fs::write(path, content).unwrap();
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
    };
    let source = paths.home.join("package");
    fs::create_dir(&source).unwrap();
    write_executable(
        &source.join("rhythm"),
        &format!("#!/bin/sh\nprintf '%s\\n' 'rhythm {VERSION}'\n"),
    );
    write_executable(
        &source.join("rhythm-ui"),
        "#!/bin/sh\n# previously installed window\n",
    );
    write_executable(
        &paths.home.join("rhythm-fixture"),
        "#!/bin/sh\nprintf '%s\n' 'rhythm 0.3.1'\n",
    );
    write_executable(
        &paths.home.join("window-fixture"),
        "#!/bin/sh\n# verified replacement window\n",
    );
    fs::write(paths.home.join("icon-fixture"), b"icns1234test").unwrap();
    let config = paths.home.join("mise-test.toml");
    let original = format!(
        "[tools]\nsample = \"1\"\n\"{TOOL}\" = {{ version = \"0.3.0\", custom = \"keep\" }}\n"
    );
    fs::write(&config, &original).unwrap();
    let fake = paths.home.join("mise-test");
    write_executable(&fake, FAKE_MISE);
    let mise = Mise {
        executable: fake,
        tool: TOOL.into(),
    };
    let mut desktop = DesktopStub {
        config: config.clone(),
        opens: false,
    };
    place(&paths, &desktop, &source.join("rhythm"), false).unwrap();
    let before = window::build(&paths.app).unwrap();
    let supervisor = Launchd::at_boundary(
        paths.home.join("empty-agents"),
        paths.home.join("not-used"),
        501,
    );
    let _guard = paths.lock().unwrap();
    let update = |desktop: &DesktopStub| {
        run_install(
            &paths,
            desktop,
            &supervisor,
            &mise,
            REPOSITORY,
            Version::new(0, 3, 1),
            true,
        )
    };
    let refused = paths.home.join("reject-release");
    fs::write(&refused, "").unwrap();
    assert!(matches!(
        update(&desktop),
        Err(Error::ReleaseVerificationFailed)
    ));
    assert_eq!(fs::read_to_string(&config).unwrap(), original);
    assert_eq!(window::build(&paths.app).unwrap(), before);
    fs::remove_file(refused).unwrap();

    assert!(matches!(update(&desktop), Err(Error::WindowUnavailable)));
    assert_eq!(installed_version(&paths).unwrap().as_deref(), Some(VERSION));
    assert_eq!(window::build(&paths.app).unwrap(), before);
    assert_eq!(
        fs::read_to_string(&config).unwrap(),
        original.replace("sample = \"1\"", "sample = \"2\"")
    );
    assert!(!paths.home.join("linked-release").exists());
    assert!(matches!(
        progress(&paths).unwrap().unwrap().phase,
        Phase::Failed {
            reason: Error::WindowUnavailable,
            rolled_back: true
        }
    ));

    // A killed helper may already have replaced one executable. The journal
    // still authorizes retrying that version even when the CLI reports it.
    fs::write(paths.area.join("current/rhythm-installation"), "0.3.1").unwrap();
    records::write(
        &paths.area.join(JOURNAL),
        &Progress {
            version: Version::new(0, 3, 1),
            phase: Phase::Installing,
        },
    )
    .unwrap();
    assert_eq!(interrupted(&paths).unwrap(), Some(Version::new(0, 3, 1)));
    desktop.opens = true;
    assert_eq!(update(&desktop).unwrap().version, "0.3.1");
    assert_ne!(window::build(&paths.app).unwrap(), before);
    assert_eq!(progress(&paths).unwrap().unwrap().phase, Phase::Succeeded);
    assert!(interrupted(&paths).unwrap().is_none());
    assert!(paths.home.join("linked-release/rhythm").is_file());
    let config = fs::read_to_string(config).unwrap();
    for exact in ["sample = \"2\"", "custom = \"keep\"", "version = \"0.3.1\""] {
        assert!(config.contains(exact));
    }
}

#[test]
fn discovery_requires_a_newer_stable_version_and_keeps_offline_offers() {
    let scratch = tempfile::tempdir().unwrap();
    let paths = Paths::new(scratch.path().canonicalize().unwrap()).unwrap();
    let guard = paths.lock().unwrap();
    let cli = paths.cli();
    fs::create_dir(cli.parent().unwrap()).unwrap();
    fs::write(cli, b"test").unwrap();
    drop(guard);
    let version = Version::new(1, 2, 3);
    let newer = Version::new(1, 3, 0);
    let published = StubCatalog {
        response: Ok(Some(newer.clone())),
        calls: Cell::new(0),
    };
    let time = Timestamp(42_000_000);
    let first = check_with(&paths, &published, false, time, &version).unwrap();
    assert_eq!(first.available, Some(newer.clone()));
    assert!(!first.offline);
    let cached = check_with(&paths, &published, false, Timestamp(time.0 + 1), &version).unwrap();
    assert_eq!(cached, first);
    assert_eq!(published.calls.get(), 1);
    let offline = StubCatalog {
        response: Err(Error::ReleaseCheckFailed),
        calls: Cell::new(0),
    };
    let lost = check_with(
        &paths,
        &offline,
        false,
        Timestamp(time.0 + HOUR_MS),
        &version,
    )
    .unwrap();
    assert!(lost.offline);
    assert_eq!(lost.available, Some(newer.clone()));
    let current = check_with(
        &paths,
        &offline,
        true,
        Timestamp(time.0 + HOUR_MS + 1),
        &newer,
    )
    .unwrap();
    assert!(current.available.is_none());
    for tag in ["1.4.0-beta.1", "1.4.0+test", "not-a-tag"] {
        assert!(catalog::stable(tag).is_none());
    }
}
