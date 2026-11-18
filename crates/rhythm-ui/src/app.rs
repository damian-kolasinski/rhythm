use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use rhythm::{
    installation::{Paths, window},
    launchd::Launchd,
    operation_error::Result,
    runs::Timestamp,
    status,
};
use rune::{App, AppSpec, Latest, Tx, Waker};

use crate::{routine::Routine, scenes, theme, updates::Updates, view};

pub struct Window {
    pub snapshot: Option<Vec<Routine>>,
    pub freshness: Option<Timestamp>,
    pub failed: bool,
    pub now: Timestamp,
    pub sample: bool,
    pub updates: Updates,
    installed: Option<InstalledWindow>,
    reports: Arc<Latest<Result<Vec<Routine>>>>,
    refresh: Option<mpsc::SyncSender<()>>,
}

impl Window {
    fn new() -> Self {
        Self {
            snapshot: None,
            freshness: None,
            failed: false,
            now: Timestamp::now(),
            sample: false,
            updates: Updates::default(),
            installed: InstalledWindow::current(),
            reports: Arc::new(Latest::default()),
            refresh: None,
        }
    }

    pub fn refresh(&mut self) {
        if let Some(refresh) = &self.refresh {
            let _ = refresh.try_send(());
        } else if self.sample && self.snapshot.is_some() {
            self.failed = false;
            self.freshness = Some(self.now);
        }
    }

    pub fn upgrade(&mut self) {
        self.updates.press();
    }

    fn apply(&mut self, result: Result<Vec<Routine>>) {
        self.now = Timestamp::now();
        self.failed = result.is_err();
        if let Ok(rows) = result {
            self.freshness = Some(self.now);
            self.snapshot = Some(rows);
        }
    }
}

fn setup(state: &mut Window, wake: Waker) {
    if state.sample {
        return;
    }
    if let Some(installed) = &state.installed
        && let Ok(Some(build)) = window::build(&installed.app)
        && let Ok(paths) = Paths::current()
    {
        state.updates.start(paths, build, wake.clone());
    }
    state.reports.set_waker(wake);
    let reports = Arc::clone(&state.reports);
    let (refresh, requests) = mpsc::sync_channel(1);
    state.refresh = Some(refresh);
    thread::spawn(move || {
        loop {
            let result = Launchd::current_user()
                .and_then(|launchd| status::routines(&launchd))
                .map(|rows| rows.into_iter().map(Routine::from).collect());
            reports.request(result, |result| result);
            if matches!(
                requests.recv_timeout(Duration::from_secs(10)),
                Err(mpsc::RecvTimeoutError::Disconnected)
            ) {
                return;
            }
        }
    });
}

fn poll(state: &mut Window, clock: f64, tx: &mut Tx) -> bool {
    let changed = state.updates.poll(clock, tx);
    let report = state.reports.take();
    if (changed || report.is_some())
        && state
            .installed
            .as_ref()
            .is_some_and(InstalledWindow::removed)
    {
        tx.quit();
    }
    if let Some(result) = report {
        state.apply(result);
        true
    } else {
        changed
    }
}

pub fn spec() -> AppSpec<Window> {
    AppSpec {
        min_size: Some((630.0, 330.0)),
        fonts: theme::FONTS,
        families: ("DM Sans", "ui-monospace"),
        scenes: scenes::SCENES,
        setup: Some(setup),
        poll: Some(poll),
        ..AppSpec::new("Rhythm", (750.0, 462.0), Window::new, view::window)
    }
}

pub fn run() {
    let mut args: Vec<_> = std::env::args().skip(1).collect();
    if args == ["--version"] {
        println!("rhythm-ui {}", rhythm::VERSION);
        return;
    }
    let mut ready = None;
    if args
        .first()
        .is_some_and(|arg| arg == window::READY_ARGUMENT)
    {
        if args.len() < 2 {
            std::process::exit(1);
        }
        ready = Some(std::path::PathBuf::from(&args[1]));
        args.drain(..2);
    }
    if App::new(spec())
        .chrome(rune::Chrome {
            full_size_content: false,
            traffic_lights: None,
            drag_regions: false,
        })
        .args(args)
        .on_frame_drawn(move |_| {
            if let Some(directory) = ready.take()
                && window::acknowledge(&directory).is_err()
            {
                std::process::exit(1);
            }
        })
        .try_run()
        .is_err()
    {
        eprintln!("Rhythm could not open its window.");
        std::process::exit(1);
    }
}

struct InstalledWindow {
    app: std::path::PathBuf,
    link: std::path::PathBuf,
    cli: std::path::PathBuf,
}

impl InstalledWindow {
    fn current() -> Option<Self> {
        let exe = std::env::current_exe().ok()?;
        let app = window::running_app(&exe)?.to_owned();
        let paths = Paths::current().ok()?;
        Some(Self {
            app,
            cli: paths.cli(),
            link: paths.link,
        })
    }

    fn removed(&self) -> bool {
        matches!(
            window::build(&self.app),
            Ok(None) | Err(rhythm::operation_error::Error::InstallationConflict)
        ) && std::fs::read_link(&self.link).as_deref().ok() != Some(&self.cli)
            && std::fs::symlink_metadata(&self.cli)
                .is_err_and(|error| error.kind() == std::io::ErrorKind::NotFound)
    }
}
