use std::{cell::Cell, os::unix::fs::MetadataExt};

use crate::launchd::Launchd;

use super::*;

struct TestDesktop {
    available: bool,
    sign_ok: bool,
    open_ok: bool,
    opened: Cell<bool>,
}

impl TestDesktop {
    fn new() -> Self {
        Self {
            available: true,
            sign_ok: true,
            open_ok: true,
            opened: Cell::new(false),
        }
    }
}

impl Desktop for TestDesktop {
    fn available(&self) -> bool {
        self.available
    }
    fn sign(&self, staged: &Path) -> Result<()> {
        assert!(staged.join("Contents/Resources/Rhythm.icns").is_file());
        self.sign_ok.then_some(()).ok_or(Error::WindowUnavailable)
    }
    fn open(&self, app: &Path, _: &Path) -> Result<()> {
        assert!(window::build(app)?.is_some());
        self.opened.set(true);
        self.open_ok.then_some(()).ok_or(Error::WindowUnavailable)
    }
}

struct Fixture {
    root: tempfile::TempDir,
    paths: Paths,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().canonicalize().unwrap().join("home");
        fs::create_dir(&home).unwrap();
        let paths = Paths::new(home).unwrap();
        Self { root, paths }
    }

    fn candidate(&self, version: &str, companion: bool) -> std::path::PathBuf {
        let root = self.root.path().join(version);
        fs::create_dir(&root).unwrap();
        let cli = root.join("rhythm");
        fs::write(
            &cli,
            format!("#!/bin/sh\nprintf '%s\\n' 'rhythm {version}'\n"),
        )
        .unwrap();
        fs::set_permissions(&cli, fs::Permissions::from_mode(0o755)).unwrap();
        if companion {
            let ui = root.join("rhythm-ui");
            fs::write(&ui, format!("#!/bin/sh\n# {version}\nexit 0\n")).unwrap();
            fs::set_permissions(ui, fs::Permissions::from_mode(0o755)).unwrap();
        }
        cli
    }

    fn supervisor(&self) -> Launchd {
        Launchd::at_boundary(
            self.paths.home.join("Library/LaunchAgents"),
            self.root.path().join("unused"),
            unsafe { libc::getuid() },
        )
    }
}

#[test]
fn installation_is_scoped_to_the_home_and_uninstalls_repeatedly() {
    let f = Fixture::new();
    let desktop = TestDesktop::new();
    let cli = f.candidate(VERSION, true);
    let result = install(&f.paths, &desktop, &cli, true).unwrap();
    assert_eq!(result.window, WindowState::Opened);
    assert!(desktop.opened.get());
    assert_eq!(fs::read_link(&f.paths.link).unwrap(), f.paths.cli());
    let app = window::build(&f.paths.app).unwrap().unwrap();
    assert_eq!(app.version, VERSION);
    assert_eq!(app.digest.len(), 64);
    let root = Root::open(f.paths.state.clone()).unwrap();
    fs::write(root.path().join("recorded-example"), b"history").unwrap();
    for _ in 0..2 {
        let removed = uninstall(&f.paths, &f.supervisor(), true).unwrap();
        assert!(matches!(removed.mise, mise::State::Kept));
        assert!(!f.paths.app.exists());
        assert!(!f.paths.link.exists());
        assert!(!f.paths.cli().exists());
        assert_eq!(
            fs::read(root.path().join("recorded-example")).unwrap(),
            b"history"
        );
    }
}

#[test]
fn conflicts_are_refused_and_window_failures_preserve_the_cli() {
    let f = Fixture::new();
    let cli = f.candidate(VERSION, true);
    let bin = f.paths.link.parent().unwrap();
    fs::create_dir_all(bin.parent().unwrap()).unwrap();
    symlink(f.root.path(), bin).unwrap();
    assert!(matches!(
        install(&f.paths, &TestDesktop::new(), &cli, false),
        Err(Error::InstallationConflict)
    ));
    fs::remove_file(bin).unwrap();
    let mut desktop = TestDesktop::new();
    desktop.sign_ok = false;
    assert_eq!(
        install(&f.paths, &desktop, &cli, true).unwrap().window,
        WindowState::Failed
    );
    assert!(f.paths.cli().exists());
    assert!(!f.paths.app.exists());
    fs::create_dir_all(&f.paths.app).unwrap();
    assert!(matches!(
        install(&f.paths, &TestDesktop::new(), &cli, false),
        Err(Error::InstallationConflict)
    ));
    assert!(f.paths.app.is_dir());
}

#[test]
fn background_and_companionless_installs_report_the_window_separately() {
    let f = Fixture::new();
    let cli = f.candidate(VERSION, false);
    let mut desktop = TestDesktop::new();
    assert_eq!(
        install(&f.paths, &desktop, &cli, false).unwrap().window,
        WindowState::NotIncluded
    );
    desktop.available = false;
    assert_eq!(
        install(&f.paths, &desktop, &cli, false).unwrap().window,
        WindowState::BackgroundSession
    );
    assert!(f.paths.cli().exists());
    assert!(!f.paths.app.exists());
}

#[test]
fn swaps_keep_the_previous_inode_and_restore_both_executables_after_failure() {
    let f = Fixture::new();
    let mut desktop = TestDesktop::new();
    let old = f.candidate(VERSION, true);
    install(&f.paths, &desktop, &old, false).unwrap();
    let open_applet = fs::File::open(f.paths.app.join("Contents/MacOS/rhythm-ui")).unwrap();
    let previous = window::build(&f.paths.app).unwrap().unwrap();
    let new = f.candidate("99.0.0", true);
    desktop.open_ok = false;
    let lock = f.paths.lock().unwrap();
    assert!(matches!(f.paths.lock(), Err(Error::InstallationBusy)));
    assert!(matches!(
        install_locked(&f.paths, &desktop, &new, "99.0.0", true, true, || Ok(())),
        Err(Error::WindowUnavailable)
    ));
    assert_eq!(window::build(&f.paths.app).unwrap(), Some(previous.clone()));
    assert_eq!(fs::read(f.paths.cli()).unwrap(), fs::read(old).unwrap());
    desktop.open_ok = true;
    assert!(matches!(
        install_locked(&f.paths, &desktop, &new, "99.0.0", false, true, || Err(
            Error::MiseUnavailable
        )),
        Err(Error::MiseUnavailable)
    ));
    assert_eq!(window::build(&f.paths.app).unwrap(), Some(previous));
    install_locked(&f.paths, &desktop, &new, "99.0.0", true, true, || Ok(())).unwrap();
    assert_ne!(
        open_applet.metadata().unwrap().ino(),
        fs::metadata(f.paths.app.join("Contents/MacOS/rhythm-ui"))
            .unwrap()
            .ino()
    );
    drop(lock);
}
