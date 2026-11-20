use rhythm::{
    authoring::OWNED_PREFIX,
    jobs::{Activation, Exit, Health, Observation, Trigger},
    routines::{CronEntry, CronTable, Ownership, Routine as DeviceRoutine, Snapshot, Source},
    runs::{Outcome, Run, RunState, Timestamp},
    status::JobReport,
    terminal_text::clean,
    time_text,
};
use std::collections::BTreeSet;

#[derive(Default)]
pub struct Inventory {
    pub rows: Vec<Routine>,
    pub incomplete: BTreeSet<Source>,
}

impl From<Snapshot> for Inventory {
    fn from(snapshot: Snapshot) -> Self {
        Self {
            rows: snapshot.routines.into_iter().map(Routine::from).collect(),
            incomplete: snapshot.incomplete,
        }
    }
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
pub enum Filter {
    #[default]
    All,
    Rhythm,
    External,
}

impl Filter {
    pub const OPTIONS: [Self; 3] = [Self::All, Self::Rhythm, Self::External];

    pub fn label(self) -> &'static str {
        match self {
            Self::All => "All",
            Self::Rhythm => "Rhythm",
            Self::External => "External",
        }
    }

    pub fn includes(self, ownership: Ownership) -> bool {
        match self {
            Self::All => true,
            Self::Rhythm => ownership != Ownership::External,
            Self::External => ownership == Ownership::External,
        }
    }
}

pub struct Routine {
    pub key: String,
    pub name: String,
    pub label: String,
    pub ownership: Ownership,
    pub state: Health,
    pub trigger: String,
    pub activity: Activity,
}

pub enum Activity {
    Recorded(Run),
    Running,
    Observed(Exit),
    NoRecording,
    NoObservation,
    Unavailable,
}

impl From<DeviceRoutine> for Routine {
    fn from(routine: DeviceRoutine) -> Self {
        let ownership = routine.ownership();
        match routine {
            DeviceRoutine::Launchd {
                source,
                instance,
                report,
                additional_timers,
            } => {
                let key = format!("{source:?}/{instance}/{}", report.label);
                Self::launchd(key, ownership, report, additional_timers)
            }
            DeviceRoutine::Cron(entry) => Self::cron(entry),
        }
    }
}

impl Routine {
    fn launchd(key: String, ownership: Ownership, job: JobReport, timers: Vec<Trigger>) -> Self {
        let label = clean(&job.label);
        let short = label
            .strip_prefix(OWNED_PREFIX)
            .unwrap_or_else(|| label.rsplit('.').next().unwrap_or(&label));
        let name = short.replace(['-', '_'], " ");
        let name = match name.char_indices().nth(1) {
            Some((end, _)) => name[..end].to_uppercase() + &name[end..],
            None => name.to_uppercase(),
        };
        let mut trigger = match job.activation {
            Some(Activation::Service { always: true }) => "Keep alive".into(),
            Some(Activation::Service { always: false }) => "Conditional keep-alive".into(),
            Some(activation) => {
                let mut text = activation.to_string();
                text.get_mut(..1).map(str::make_ascii_uppercase);
                text
            }
            None => "Unavailable".into(),
        };
        for timer in timers {
            use std::fmt::Write;
            write!(trigger, ", {timer}").expect("writing to a String");
        }
        let activity = match (job.observation, job.last_run) {
            (Observation::Loaded { pid: Some(_), .. }, Some(run))
                if run.state == RunState::Running =>
            {
                Activity::Recorded(run)
            }
            (Observation::Loaded { pid: Some(_), .. }, _) => Activity::Running,
            (_, Some(run)) => Activity::Recorded(run),
            (
                Observation::Loaded {
                    last_exit: Some(exit),
                    ..
                },
                _,
            ) => Activity::Observed(exit),
            (Observation::Unavailable, _) => Activity::Unavailable,
            _ if job.recorded => Activity::NoRecording,
            _ => Activity::NoObservation,
        };
        Self {
            key,
            name,
            label,
            ownership,
            state: job.health,
            trigger,
            activity,
        }
    }

    fn cron(entry: CronEntry) -> Self {
        let table = match entry.table {
            CronTable::User => "User crontab".into(),
            CronTable::System => "System crontab".into(),
            CronTable::Other(index) => format!("Other crontab {index}"),
        };
        Self {
            key: format!("cron/{:?}/{}", entry.table, entry.line),
            name: "Cron job".into(),
            label: format!("{table} · line {}", entry.line),
            ownership: Ownership::External,
            state: Health::Unknown,
            trigger: entry.schedule,
            activity: Activity::Unavailable,
        }
    }
}

impl Activity {
    pub fn text(&self, now: Timestamp) -> (String, Option<String>) {
        match self {
            Self::NoRecording => ("No recorded runs".into(), None),
            Self::NoObservation => ("No observed runs".into(), None),
            Self::Unavailable => ("Unavailable".into(), None),
            Self::Running => ("Running".into(), None),
            Self::Observed(exit) => (
                match exit {
                    exit if exit.succeeded() => "Succeeded".into(),
                    Exit::Code { code } => format!("Exit {code}"),
                    exit => exit.to_string(),
                },
                Some("Time unavailable".into()),
            ),
            Self::Recorded(run) => match run.state {
                RunState::Running => (
                    format!("Started {}", time_text::ago(run.started_at, now)),
                    None,
                ),
                RunState::Lost => (
                    "Run interrupted".into(),
                    Some(time_text::ago(run.started_at, now)),
                ),
                RunState::Finished {
                    ended_at, outcome, ..
                } => {
                    let headline = match outcome {
                        Outcome::Code { code: 0 } => "Succeeded".into(),
                        Outcome::Code { code } => format!("Exit {code}"),
                        _ => outcome.to_string(),
                    };
                    let timing = format!(
                        "{} · {}",
                        time_text::ago(ended_at, now),
                        time_text::duration(ended_at.0.saturating_sub(run.started_at.0)),
                    );
                    (headline, Some(timing))
                }
            },
        }
    }
}
