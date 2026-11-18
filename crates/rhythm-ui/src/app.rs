use std::{
    sync::{Arc, mpsc},
    thread,
    time::Duration,
};

use rhythm::{launchd::Launchd, operation_error::Result, runs::Timestamp, status};
use rune::{App, AppSpec, Latest, Tx, Waker};

use crate::{routine::Routine, scenes, theme, view};

pub struct Window {
    pub snapshot: Option<Vec<Routine>>,
    pub freshness: Option<Timestamp>,
    pub failed: bool,
    pub now: Timestamp,
    pub sample: bool,
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

fn poll(state: &mut Window, _: f64, _: &mut Tx) -> bool {
    let Some(result) = state.reports.take() else {
        return false;
    };
    state.apply(result);
    true
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
    if App::new(spec())
        .chrome(rune::Chrome {
            full_size_content: false,
            traffic_lights: None,
            drag_regions: false,
        })
        .args(std::env::args().skip(1))
        .try_run()
        .is_err()
    {
        eprintln!("Rhythm could not open its window.");
        std::process::exit(1);
    }
}
