use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use rhythm::{
    installation::{Paths, window},
    operation_error::Result,
    routines,
    runs::Timestamp,
};
use rune::{App, AppSpec, Latest, Tx, Waker};

use crate::{
    routine::{Filter, Inventory, Routine},
    scenes, theme,
    updates::Updates,
    view,
};

pub struct Window {
    pub snapshot: Option<Inventory>,
    pub filter: Filter,
    pub freshness: Option<Timestamp>,
    pub failed: bool,
    pub now: Timestamp,
    pub sample: bool,
    pub updates: Updates,
    /// The key of the routine whose problems the dialog shows.
    pub problems: Option<String>,
    installed: Option<InstalledWindow>,
    reports: Arc<Latest<Result<Inventory>>>,
    refresh: Option<mpsc::SyncSender<()>>,
}

impl Window {
    fn new() -> Self {
        Self {
            snapshot: None,
            filter: Filter::All,
            freshness: None,
            failed: false,
            now: Timestamp::now(),
            sample: false,
            updates: Updates::default(),
            problems: None,
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

    /// Opens the dialog for the routine keyed `key` while it has problems
    /// open; a tap that lands after a refresh resolved them opens nothing.
    pub fn show_problems(&mut self, key: &str) {
        let previous = self.problems.replace(key.to_owned());
        if self.inspected().is_none() {
            self.problems = previous;
        }
    }

    pub fn close_problems(&mut self) {
        self.problems = None;
    }

    /// The routine the problems dialog shows, while it has problems open.
    pub fn inspected(&self) -> Option<&Routine> {
        let key = self.problems.as_deref()?;
        self.snapshot
            .as_ref()?
            .rows
            .iter()
            .find(|routine| routine.key == key && !routine.problems.is_empty())
    }

    fn apply(&mut self, result: Result<Inventory>) {
        self.now = Timestamp::now();
        self.failed = result.is_err();
        if let Ok(rows) = result {
            self.freshness = Some(self.now);
            self.snapshot = Some(rows);
            // The dialog closes once its routine has no problems open or is gone.
            if self.inspected().is_none() {
                self.problems = None;
            }
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
            let result = routines::current().map(Inventory::from);
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

#[cfg(test)]
mod tests {
    use super::*;
    use rhythm::operation_error::Error;

    fn scene(name: &str) -> Window {
        let mut window = Window::new();
        let (_, setup) = scenes::SCENES.iter().find(|(n, _)| *n == name).unwrap();
        setup(&mut window);
        window
    }

    fn inventory(name: &str) -> Inventory {
        scene(name).snapshot.unwrap()
    }

    fn shown(window: &Window) -> Option<usize> {
        window.inspected().map(|routine| routine.problems.len())
    }

    #[test]
    fn the_problems_dialog_follows_each_refresh_and_closes_once_resolved() {
        let mut window = scene("problems");
        let key = window.problems.clone().unwrap();
        assert_eq!(shown(&window), Some(2));

        window.apply(Ok(inventory("problems-scroll")));
        assert_eq!(
            shown(&window),
            Some(100),
            "a refresh shows what is open now"
        );

        window.apply(Err(Error::StorageUnavailable));
        assert_eq!(
            shown(&window),
            Some(100),
            "a failed refresh keeps the last view"
        );

        let mut resolved = inventory("problems");
        for routine in &mut resolved.rows {
            if routine.key == key {
                routine.problems.clear();
            }
        }
        window.apply(Ok(resolved));
        assert_eq!((window.problems.as_deref(), shown(&window)), (None, None));

        window.show_problems(&key);
        assert_eq!(
            window.problems, None,
            "a routine without problems opens nothing"
        );

        let mut window = scene("problems");
        window.apply(Ok(inventory("routines")));
        assert_eq!(window.problems, None, "the routine is gone");
    }
}
