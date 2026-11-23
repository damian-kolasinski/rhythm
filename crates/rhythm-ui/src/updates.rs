//! The update worker runs on its own schedule, independent of inventory reads.
use std::{
    path::PathBuf,
    sync::{Arc, mpsc},
    time::{Duration, Instant},
};

use rhythm::{
    VERSION,
    installation::{
        Paths,
        update::{self, Check, Phase, Progress},
        window::{self, Build},
    },
    operation_error::{Error, Result},
};
use rune::{Latest, Tx, Waker};
use semver::Version;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum Offer {
    Release { version: Version, offline: bool },
    Staged(Version),
    Repair(Version),
}

impl Offer {
    fn version(&self) -> &Version {
        match self {
            Self::Release { version, .. } | Self::Staged(version) | Self::Repair(version) => {
                version
            }
        }
    }
}

#[derive(Default)]
pub struct Updates {
    pub offer: Option<Offer>,
    pub working: bool,
    pub error: Option<Error>,
    pub progress: Option<Progress>,
    pub sample: bool,
    sample_at: Option<f64>,
    sample_version: Option<Version>,
    cli: Option<PathBuf>,
    feed: Arc<Latest<Report>>,
    result: Latest<Result<()>>,
    stop: Option<mpsc::Sender<()>>,
}

#[derive(Clone, Eq, PartialEq)]
struct Report {
    offer: Option<Offer>,
    progress: Option<Progress>,
}

impl Updates {
    pub fn start(&mut self, paths: Paths, running: Build, wake: Waker) {
        self.cli = Some(paths.cli());
        self.feed.set_waker(wake.clone());
        self.result.set_waker(wake);
        let feed = Arc::clone(&self.feed);
        let (stop, stopped) = mpsc::channel();
        self.stop = Some(stop);
        std::thread::spawn(move || {
            let current = running
                .version
                .parse()
                .unwrap_or_else(|_| Version::new(0, 0, 0));
            let mut check = None;
            let mut next_check = Instant::now();
            let mut prior = None;
            loop {
                if Instant::now() >= next_check {
                    if let Ok(answer) = update::check(&paths, false) {
                        check = Some(answer);
                    }
                    next_check = Instant::now() + Duration::from_secs(3600);
                }
                let staged = window::build(&paths.app)
                    .ok()
                    .flatten()
                    .filter(|build| build.digest != running.digest)
                    .and_then(|build| build.version.parse::<Version>().ok())
                    .filter(|version| version >= &current);
                let recovery = update::recovery(&paths).ok().flatten();
                let report = Report {
                    offer: recovery
                        .map(Offer::Repair)
                        .or_else(|| offer(check.as_ref(), staged)),
                    progress: update::progress(&paths).ok().flatten(),
                };
                if prior.as_ref() != Some(&report) {
                    prior = Some(report.clone());
                    feed.request(report, |report| report);
                }
                if !matches!(
                    stopped.recv_timeout(Duration::from_secs(1)),
                    Err(mpsc::RecvTimeoutError::Timeout)
                ) {
                    break;
                }
            }
        });
    }

    pub fn press(&mut self) {
        let Some(offer) = &self.offer else { return };
        if self.working {
            return;
        }
        self.error = None;
        self.progress = None;
        if self.sample {
            self.working = true;
            self.sample_at = None;
            return;
        }
        let Some(cli) = &self.cli else { return };
        self.working = true;
        let version = match offer {
            Offer::Release { version, .. } | Offer::Repair(version) => Some(version.clone()),
            Offer::Staged(_) => None,
        };
        self.result
            .request((cli.clone(), version), |(cli, expected)| {
                update::upgrade_window(&cli, expected.as_ref())
            });
    }

    pub fn poll(&mut self, clock: f64, tx: &mut Tx) -> bool {
        if self.sample && self.working {
            return self.demo(clock, tx);
        }
        let mut changed = false;
        if let Some(report) = self.feed.take() {
            if !self.working {
                self.offer = report.offer;
            }
            self.progress = report.progress;
            changed = true;
        }
        if let Some(result) = self.result.take() {
            self.working = false;
            match result {
                Ok(()) => tx.quit(),
                Err(error) => self.error = Some(error),
            }
            changed = true;
        }
        changed
    }

    fn demo(&mut self, clock: f64, tx: &mut Tx) -> bool {
        let start = *self.sample_at.get_or_insert(clock);
        let elapsed = clock - start;
        let (phase, next) = if elapsed < 1.5 {
            (Phase::Downloading, 1.5)
        } else if elapsed < 3.0 {
            (Phase::Reopening, 3.0)
        } else {
            self.working = false;
            self.sample_version = self.offer.take().map(|offer| offer.version().clone());
            self.progress = None;
            return true;
        };
        let version = self.offer.as_ref().unwrap().version().clone();
        let progress = Progress { version, phase };
        let changed = self.progress.as_ref() != Some(&progress);
        self.progress = Some(progress);
        tx.wake_at(start + next);
        changed
    }

    pub fn title(&self) -> Option<String> {
        let offer = self.offer.as_ref()?;
        Some(if self.working {
            format!("Upgrading Rhythm to {}", offer.version())
        } else if self.error.is_some() {
            "Couldn’t upgrade Rhythm".into()
        } else {
            format!("Rhythm {} is available", offer.version())
        })
    }

    pub fn message(&self) -> &'static str {
        if self.working {
            return self
                .progress
                .as_ref()
                .filter(|job| !job.phase.terminal() && self.matches(job))
                .map_or("Preparing the upgrade…", |job| job.phase.label());
        }
        if let Some(error) = self.error {
            return error.message();
        }
        match self.offer {
            Some(Offer::Release { offline: true, .. }) => {
                "Last checked version. Couldn’t reach GitHub."
            }
            Some(Offer::Staged(_)) => "A newer build is installed and ready to open.",
            Some(Offer::Repair(_)) => {
                "Resume the interrupted update to finish installing both components."
            }
            _ => "Updates the app and command line together.",
        }
    }

    fn matches(&self, job: &Progress) -> bool {
        self.offer
            .as_ref()
            .is_some_and(|offer| offer.version() == &job.version)
    }

    pub fn set_sample_version(&mut self, version: Version) {
        self.sample_version = Some(version);
    }

    pub fn displayed_version(&self) -> String {
        self.sample_version
            .as_ref()
            .map_or_else(|| VERSION.into(), ToString::to_string)
    }
}

fn offer(check: Option<&Check>, staged: Option<Version>) -> Option<Offer> {
    if let Some(check) = check
        && let Some(version) = &check.available
        && staged.as_ref().is_none_or(|staged| version > staged)
    {
        return Some(Offer::Release {
            version: version.clone(),
            offline: check.offline,
        });
    }
    staged.map(Offer::Staged)
}
