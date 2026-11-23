use std::{
    collections::BTreeSet,
    path::Path,
    sync::{Arc, mpsc},
    thread,
    time::{Duration, Instant},
};

use rhythm::{
    installation::{Paths, window},
    jobs::Label,
    operation_error::Result,
    routines,
    runs::Timestamp,
};
use rune::{App, AppSpec, Latest, Tx, Waker};

use crate::{
    finder::{self, Finder},
    removal::{Flow, Next, Removed, Reply},
    routine::{Filter, Inventory, Routine},
    routine_control::{self, RoutineControl},
    routine_menu, scenes, theme,
    updates::Updates,
    view,
};

/// How long a sample removal takes, so the demonstration shows it running.
const SAMPLE_REMOVAL: f64 = 0.8;

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
    /// The routine whose actions menu is open.
    pub menu: Option<routine_menu::Open>,
    /// The routine the removal dialog is about.
    pub removal: Option<Flow>,
    /// The routine removed last, until the operator dismisses the notice.
    pub removed: Option<Removed>,
    /// Rows of removed routines, which a refresh begun before the removal
    /// may still list.
    hidden: Option<Hidden>,
    pub control: Arc<dyn RoutineControl>,
    pub finder: Box<dyn Finder>,
    replies: Arc<Latest<Reply>>,
    /// A sample removal's reply and the clock time it lands at.
    held: Option<(Reply, Option<f64>)>,
    installed: Option<InstalledWindow>,
    /// Each refresh's result, with when it began reading.
    reports: Arc<Latest<(Instant, Result<Inventory>)>>,
    refresh: Option<mpsc::SyncSender<()>>,
}

struct Hidden {
    keys: BTreeSet<String>,
    /// When the newest of them was removed.
    since: Instant,
}

enum Ask {
    Read(Label),
    Remove(Label),
}

fn answer(control: &dyn RoutineControl, ask: Ask) -> Reply {
    match ask {
        Ask::Read(label) => {
            let result = control.removal(&label);
            Reply::Read(label, result)
        }
        Ask::Remove(label) => {
            let result = control.remove(&label);
            Reply::Removed(label, result)
        }
    }
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
            menu: None,
            removal: None,
            removed: None,
            hidden: None,
            control: Arc::new(routine_control::Launchd),
            finder: Box::new(finder::System),
            replies: Arc::new(Latest::default()),
            held: None,
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

    fn routine(&self, key: &str) -> Option<&Routine> {
        self.snapshot
            .as_ref()?
            .rows
            .iter()
            .find(|routine| routine.key == key)
    }

    /// Opens the actions menu under the row's button, or closes it.
    pub fn toggle_menu(&mut self, key: &str) {
        let open = routine_menu::Open {
            key: key.to_owned(),
            at: None,
        };
        self.menu = (self.menu.as_ref() != Some(&open)).then_some(open);
    }

    /// Opens the actions menu where a secondary click landed on the row.
    pub fn menu_at(&mut self, key: &str, x: f32, y: f32) {
        if self
            .routine(key)
            .is_some_and(|routine| routine.removable.is_some())
        {
            self.menu = Some(routine_menu::Open {
                key: key.to_owned(),
                at: Some((x, y)),
            });
        }
    }

    pub fn close_menu(&mut self) {
        self.menu = None;
    }

    /// Opens the removal dialog for a routine the window can remove and reads
    /// what removing it would leave behind.
    pub fn ask_remove(&mut self, key: &str) {
        self.menu = None;
        if self.removal.as_ref().is_some_and(|flow| !flow.closable()) {
            return;
        }
        let Some(flow) = self.routine(key).and_then(Flow::open) else {
            return;
        };
        let label = flow.label.clone();
        self.removal = Some(flow);
        self.dispatch(Ask::Read(label));
    }

    pub fn confirm_remove(&mut self) {
        if let Some(Next::Remove(label)) = self.removal.as_mut().map(Flow::confirm) {
            self.dispatch(Ask::Remove(label));
        }
    }

    pub fn cancel_remove(&mut self) {
        if self.removal.as_ref().is_some_and(Flow::closable) {
            self.removal = None;
        }
    }

    pub fn reveal(&mut self, path: &Path) {
        self.finder.reveal(path);
    }

    pub fn dismiss_removed(&mut self) {
        self.removed = None;
    }

    /// Asks the machine off the window's thread; samples answer at once, and
    /// a sample removal lands after a moment.
    fn dispatch(&mut self, ask: Ask) {
        let control = Arc::clone(&self.control);
        if !self.sample {
            self.replies
                .request((control, ask), |(control, ask)| answer(&*control, ask));
            return;
        }
        match answer(&*control, ask) {
            reply @ Reply::Removed(..) => self.held = Some((reply, None)),
            reply => self.receive(reply),
        }
    }

    fn receive(&mut self, reply: Reply) {
        let Some(flow) = self.removal.as_mut() else {
            return;
        };
        if let Next::Gone(key, removed) = flow.receive(reply) {
            self.removal = None;
            if let Some(inventory) = self.snapshot.as_mut() {
                inventory.rows.retain(|routine| routine.key != key);
            }
            let hidden = self.hidden.get_or_insert_with(|| Hidden {
                keys: BTreeSet::new(),
                since: Instant::now(),
            });
            hidden.keys.insert(key);
            hidden.since = Instant::now();
            self.removed = Some(removed);
            self.refresh();
        }
    }

    /// Shows the result of a refresh that began reading at `started`.
    fn apply(&mut self, started: Instant, result: Result<Inventory>) {
        self.now = Timestamp::now();
        self.failed = result.is_err();
        if let Ok(mut rows) = result {
            // A refresh begun after the removals shows the machine as it is.
            match &self.hidden {
                Some(hidden) if started < hidden.since => rows
                    .rows
                    .retain(|routine| !hidden.keys.contains(&routine.key)),
                _ => self.hidden = None,
            }
            self.freshness = Some(self.now);
            self.snapshot = Some(rows);
            // The dialog closes once its routine has no problems open or is gone.
            if self.inspected().is_none() {
                self.problems = None;
            }
            if let Some(open) = &self.menu
                && self.routine(&open.key).is_none()
            {
                self.menu = None;
            }
            // A routine removed elsewhere has nothing left to remove.
            if let Some(flow) = &self.removal
                && flow.closable()
                && self.routine(&flow.key).is_none()
            {
                self.removal = None;
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
    state.replies.set_waker(wake.clone());
    state.reports.set_waker(wake);
    let reports = Arc::clone(&state.reports);
    let (refresh, requests) = mpsc::sync_channel(1);
    state.refresh = Some(refresh);
    thread::spawn(move || {
        loop {
            let started = Instant::now();
            let result = routines::current().map(Inventory::from);
            reports.request((started, result), |report| report);
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
    let mut changed = state.updates.poll(clock, tx);
    if let Some(reply) = state.replies.take() {
        state.receive(reply);
        changed = true;
    }
    if let Some((_, due)) = &mut state.held {
        let due = *due.get_or_insert(clock + SAMPLE_REMOVAL);
        if clock >= due {
            let (reply, _) = state.held.take().expect("a held reply");
            state.receive(reply);
            changed = true;
        } else {
            tx.wake_at(due);
        }
    }
    let report = state.reports.take();
    if (changed || report.is_some())
        && state
            .installed
            .as_ref()
            .is_some_and(InstalledWindow::removed)
    {
        tx.quit();
    }
    if let Some((started, result)) = report {
        state.apply(started, result);
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
    use crate::removal::Stage;
    use rhythm::{authoring::Removal, operation_error::Error};
    use std::path::PathBuf;

    const ARCHIVE: &str = "UserAgent/0/dev.rhythm.archive-samples";
    const SCRIPT: &str = "/Users/example/scripts/archive-samples.sh";

    fn listed(window: &Window, key: &str) -> bool {
        window.routine(key).is_some()
    }

    /// Lands the sample removal the window is waiting for.
    fn land(window: &mut Window) {
        let (reply, _) = window.held.take().expect("a removal in progress");
        window.receive(reply);
    }

    #[test]
    fn a_removed_routine_stays_hidden_until_a_refresh_no_longer_lists_it() {
        let mut window = scene("routines");
        let finder = finder::Sample::default();
        window.finder = Box::new(finder.clone());
        window.ask_remove(ARCHIVE);
        assert!(
            matches!(&window.removal.as_ref().unwrap().stage, Stage::Ready(Removal { script: Some(s) }) if s.to_str() == Some(SCRIPT))
        );
        window.reveal(Path::new(SCRIPT));
        assert_eq!(*finder.revealed.borrow(), [PathBuf::from(SCRIPT)]);

        window.confirm_remove();
        window.cancel_remove();
        assert!(
            window.removal.is_some(),
            "a removal in progress shows its outcome"
        );
        window.apply(Instant::now(), Ok(inventory("routines")));
        assert!(window.removal.is_some() && listed(&window, ARCHIVE));

        let before = Instant::now();
        land(&mut window);
        assert!(window.removal.is_none());
        assert!(!listed(&window, ARCHIVE));
        assert_eq!(
            window.removed,
            Some(Removed {
                name: "Archive samples".into(),
                script: Some(SCRIPT.into()),
            })
        );

        window.apply(before, Ok(inventory("routines")));
        assert!(
            !listed(&window, ARCHIVE),
            "a refresh begun before the removal"
        );
        window.apply(Instant::now(), Ok(inventory("routines")));
        assert!(
            listed(&window, ARCHIVE),
            "a refresh begun after it shows the routine if it is back"
        );
    }

    #[test]
    fn a_label_two_definitions_share_cannot_be_removed() {
        let mut window = scene("routines");
        let mut snapshot = rhythm::routines::Snapshot {
            routines: Vec::new(),
            incomplete: Default::default(),
        };
        scenes::push_shared_label(&mut snapshot);
        window.apply(Instant::now(), Ok(snapshot.into()));
        let rows = &window.snapshot.as_ref().unwrap().rows;
        assert_eq!(rows.len(), 2);
        assert!(rows.iter().all(|routine| routine.removable.is_none()));
    }

    struct Failing;
    impl RoutineControl for Failing {
        fn removal(&self, _: &Label) -> Result<Removal> {
            Ok(Removal { script: None })
        }
        fn remove(&self, _: &Label) -> Result<Removal> {
            Err(Error::SupervisorUnavailable)
        }
    }

    #[test]
    fn a_failed_removal_keeps_the_routine_to_try_again() {
        let mut window = scene("routines");
        window.control = Arc::new(Failing);
        window.ask_remove(ARCHIVE);
        window.confirm_remove();
        land(&mut window);
        assert!(matches!(
            window.removal.as_ref().unwrap().stage,
            Stage::Failed(_, Error::SupervisorUnavailable)
        ));
        assert!(listed(&window, ARCHIVE) && window.removed.is_none());
        window.confirm_remove();
        assert!(matches!(
            window.removal.as_ref().unwrap().stage,
            Stage::Removing(_)
        ));
    }

    #[test]
    fn only_routines_rhythm_records_can_be_removed() {
        let mut window = scene("routines");
        window.menu_at("Daemon/0/com.example.sync-catalog", 10.0, 10.0);
        assert!(window.menu.is_none());
        window.ask_remove("Daemon/0/com.example.sync-catalog");
        assert!(window.removal.is_none());

        window.toggle_menu(ARCHIVE);
        assert!(window.menu.is_some());
        window.toggle_menu(ARCHIVE);
        assert!(window.menu.is_none());

        // A routine whose definition is gone or no longer recorded.
        window.control = Arc::new(routine_control::Sample::default());
        window.ask_remove(ARCHIVE);
        assert!(matches!(
            window.removal.as_ref().unwrap().stage,
            Stage::Unreadable(Error::JobNotFound)
        ));
        window.confirm_remove();
        assert!(
            window.held.is_none(),
            "nothing to remove without knowing what stays"
        );
        window.apply(Instant::now(), Ok(inventory("removed")));
        assert!(
            window.removal.is_none(),
            "it closes once the routine is gone"
        );
    }

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

        window.apply(Instant::now(), Ok(inventory("problems-scroll")));
        assert_eq!(
            shown(&window),
            Some(100),
            "a refresh shows what is open now"
        );

        window.apply(Instant::now(), Err(Error::StorageUnavailable));
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
        window.apply(Instant::now(), Ok(resolved));
        assert_eq!((window.problems.as_deref(), shown(&window)), (None, None));

        window.show_problems(&key);
        assert_eq!(
            window.problems, None,
            "a routine without problems opens nothing"
        );

        let mut window = scene("problems");
        window.apply(Instant::now(), Ok(inventory("routines")));
        assert_eq!(window.problems, None, "the routine is gone");
    }
}
