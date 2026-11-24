use rhythm::{
    authoring::OWNED_PREFIX,
    findings::{Finding, Stated},
    findings_render,
    jobs::{Activation, Exit, Health, Label, Observation, RunStore, Trigger},
    routines::{CronEntry, CronTable, Ownership, Routine as DeviceRoutine, Snapshot, Source},
    runs::{Outcome, Run, RunState, Timestamp},
    sessions::SessionRun,
    status::JobReport,
    terminal_text::clean,
    time_text,
};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Default)]
pub struct Inventory {
    pub rows: Vec<Routine>,
    pub incomplete: BTreeSet<Source>,
}

impl From<Snapshot> for Inventory {
    fn from(snapshot: Snapshot) -> Self {
        // Removal names a job by its label, which must say which routine.
        let mut labels = BTreeMap::<String, usize>::new();
        for routine in &snapshot.routines {
            if let DeviceRoutine::Launchd { report, .. } = routine {
                *labels.entry(report.label.clone()).or_default() += 1;
            }
        }
        let mut rows: Vec<Routine> = snapshot.routines.into_iter().map(Routine::from).collect();
        for row in &mut rows {
            if row
                .removable
                .as_ref()
                .is_some_and(|label| labels.get(label.as_str()) != Some(&1))
            {
                row.removable = None;
            }
        }
        Self {
            rows,
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
    /// The problems the routine reports as open, as `rhythm findings` lists
    /// them.
    pub problems: Vec<Finding>,
    /// Problems the routine listed beyond the first 100, which are not kept.
    pub unlisted: u32,
    /// The launchd label of a routine Rhythm created or adopted, which the
    /// window can remove; `None` for every other routine.
    pub removable: Option<Label>,
    pub run_store: Option<RunStore>,
    pub session_run: Option<SessionRun>,
}

pub enum Activity {
    /// The newest real run, and when the newest skipped run that followed it
    /// ended.
    Recorded {
        run: Run,
        checked: Option<Timestamp>,
    },
    /// Every recorded run so far was skipped; the newest ended then.
    Checked(Timestamp),
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
                findings,
            } => {
                let key = format!("{source:?}/{instance}/{}", report.label);
                Self::launchd(key, ownership, report, additional_timers, findings)
            }
            DeviceRoutine::Cron(entry) => Self::cron(entry),
        }
    }
}

impl Routine {
    fn launchd(
        key: String,
        ownership: Ownership,
        job: JobReport,
        timers: Vec<Trigger>,
        stated: Option<Stated>,
    ) -> Self {
        let removable = (ownership != Ownership::External)
            .then(|| job.label.parse().ok())
            .flatten();
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
        let (problems, unlisted) = stated.map_or((Vec::new(), 0), |stated| {
            (stated.set.open, stated.set.dropped)
        });
        let checked = job.last_skip.map(|run| match run.state {
            RunState::Finished { ended_at, .. } => ended_at,
            _ => run.started_at,
        });
        let activity = match (job.observation, job.last_run, checked) {
            (Observation::Loaded { pid: Some(_), .. }, Some(run), _)
                if run.state == RunState::Running =>
            {
                Activity::Recorded { run, checked }
            }
            (Observation::Loaded { pid: Some(_), .. }, ..) => Activity::Running,
            (_, Some(run), _) => Activity::Recorded { run, checked },
            (_, None, Some(at)) => Activity::Checked(at),
            (
                Observation::Loaded {
                    last_exit: Some(exit),
                    ..
                },
                ..,
            ) => Activity::Observed(exit),
            (Observation::Unavailable, ..) => Activity::Unavailable,
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
            problems,
            unlisted,
            removable,
            run_store: job.run_store,
            session_run: job.last_session_run,
        }
    }

    pub fn session_is_latest(&self) -> bool {
        matches!((&self.activity, self.session_run),
            (Activity::Recorded { run, .. }, Some(sessions))
            if run.id == sessions.id && matches!(run.state, RunState::Finished { .. }))
    }

    /// What the Latest activity cell says: the open problems when there are
    /// any, otherwise the newest real run's outcome; then when it happened.
    pub fn latest(&self, now: Timestamp) -> Latest {
        let Said {
            outcome,
            at,
            took,
            checked,
        } = self.activity.said(now);
        let checked = checked.map(|at| format!("checked {}", time_text::ago(at, now)));
        if self.problems.is_empty() {
            let at = match at {
                When::At(at) => Some(time_text::ago(at, now)),
                When::Unknown => Some("Time unavailable".into()),
                When::Unsaid => None,
            };
            return Latest::new(outcome, [at, took, checked]);
        }
        let outcome = match at {
            When::At(at) => format!("{outcome} {}", time_text::ago(at, now)),
            When::Unknown | When::Unsaid => outcome,
        };
        Latest::new(
            findings_render::problems(self.problems.len()),
            [Some(outcome), checked],
        )
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
            problems: Vec::new(),
            unlisted: 0,
            removable: None,
            run_store: None,
            session_run: None,
        }
    }
}

/// The Latest activity cell: a headline and, under it, a quieter line.
#[derive(Debug, PartialEq, Eq)]
pub struct Latest {
    pub headline: String,
    pub detail: Option<String>,
}

impl Latest {
    fn new<const N: usize>(headline: String, parts: [Option<String>; N]) -> Self {
        // A narrow cell wraps between the parts, never inside one.
        let parts: Vec<_> = parts
            .into_iter()
            .flatten()
            .map(|part| part.replace(' ', "\u{a0}"))
            .collect();
        Self {
            headline,
            detail: (!parts.is_empty()).then(|| parts.join(" · ")),
        }
    }
}

/// What an activity says, before it is laid out in the cell.
struct Said {
    outcome: String,
    at: When,
    took: Option<String>,
    checked: Option<Timestamp>,
}

enum When {
    At(Timestamp),
    /// launchd knows the last exit but not when it happened.
    Unknown,
    Unsaid,
}

impl Activity {
    fn said(&self, now: Timestamp) -> Said {
        let only = |outcome: &str| Said {
            outcome: outcome.into(),
            at: When::Unsaid,
            took: None,
            checked: None,
        };
        match self {
            Self::NoRecording => only("No recorded runs"),
            Self::NoObservation => only("No observed runs"),
            Self::Unavailable => only("Unavailable"),
            Self::Running => only("Running"),
            Self::Checked(at) => Said {
                at: When::At(*at),
                ..only("Checked")
            },
            Self::Observed(exit) => Said {
                at: When::Unknown,
                ..only(&match exit {
                    exit if exit.succeeded() => "Succeeded".into(),
                    Exit::Code { code } => format!("Exit {code}"),
                    exit => exit.to_string(),
                })
            },
            Self::Recorded { run, checked } => {
                let checked = *checked;
                match run.state {
                    RunState::Running => Said {
                        checked,
                        ..only(&format!("Started {}", time_text::ago(run.started_at, now)))
                    },
                    RunState::Lost => Said {
                        at: When::At(run.started_at),
                        checked,
                        ..only("Run interrupted")
                    },
                    RunState::Finished {
                        ended_at, outcome, ..
                    } => Said {
                        outcome: match outcome {
                            Outcome::Code { code: 0 } => "Succeeded".into(),
                            Outcome::Code { code } => format!("Exit {code}"),
                            _ => outcome.to_string(),
                        },
                        at: When::At(ended_at),
                        took: Some(time_text::duration(
                            ended_at.0.saturating_sub(run.started_at.0),
                        )),
                        checked,
                    },
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rhythm::findings::{FindingKey, Problem};

    const NOW: Timestamp = Timestamp(100_000_000);
    const HOURS_3: i64 = 10_800_000;
    const MINUTES_4: i64 = 240_000;

    fn run(state: RunState, started: i64) -> Run {
        Run {
            id: 1,
            label: "dev.rhythm.example".into(),
            started_at: Timestamp(NOW.0 - started),
            state,
        }
    }

    fn finished(code: i32, ago: i64, took: i64) -> Run {
        let state = RunState::Finished {
            ended_at: Timestamp(NOW.0 - ago),
            outcome: Outcome::Code { code },
            report: Default::default(),
        };
        run(state, ago + took)
    }

    fn checked(ago: i64) -> Option<Timestamp> {
        Some(Timestamp(NOW.0 - ago))
    }

    /// The cell's two lines, with ordinary spaces.
    fn latest(activity: Activity, problems: usize) -> (String, Option<String>) {
        let problem = |n| Finding {
            problem: Problem {
                key: FindingKey::parse(&format!("problem-{n}")).unwrap(),
                title: String::from("A problem").into(),
                detail: None,
                link: None,
                handled_by: None,
            },
            first_seen: NOW,
        };
        let routine = Routine {
            key: "key".into(),
            name: "Example".into(),
            label: "dev.rhythm.example".into(),
            ownership: Ownership::Created,
            state: Health::Ok,
            trigger: "Every 15m".into(),
            activity,
            problems: (0..problems).map(problem).collect(),
            unlisted: 0,
            removable: None,
            run_store: None,
            session_run: None,
        };
        let Latest { headline, detail } = routine.latest(NOW);
        (headline, detail.map(|d| d.replace('\u{a0}', " ")))
    }

    fn lines(headline: &str, detail: Option<&str>) -> (String, Option<String>) {
        (headline.into(), detail.map(Into::into))
    }

    #[test]
    fn a_real_run_says_how_it_ended_and_when_later_skips_checked() {
        let succeeded = || finished(0, HOURS_3, 41_000);
        let activity = |checked| Activity::Recorded {
            run: succeeded(),
            checked,
        };
        assert_eq!(
            latest(activity(None), 0),
            lines("Succeeded", Some("3h ago · 41.0s"))
        );
        assert_eq!(
            latest(activity(checked(MINUTES_4)), 0),
            lines("Succeeded", Some("3h ago · 41.0s · checked 4m ago"))
        );
        assert_eq!(
            latest(activity(checked(MINUTES_4)), 2),
            lines("2 problems", Some("Succeeded 3h ago · checked 4m ago"))
        );
        let failed = Activity::Recorded {
            run: finished(1, HOURS_3, 2_000),
            checked: None,
        };
        assert_eq!(latest(failed, 1), lines("1 problem", Some("Exit 1 3h ago")));
    }

    #[test]
    fn runs_without_an_outcome_say_what_is_known() {
        let lost = |checked| Activity::Recorded {
            run: run(RunState::Lost, HOURS_3),
            checked,
        };
        assert_eq!(
            latest(lost(checked(MINUTES_4)), 0),
            lines("Run interrupted", Some("3h ago · checked 4m ago"))
        );
        assert_eq!(
            latest(lost(None), 1),
            lines("1 problem", Some("Run interrupted 3h ago"))
        );
        let started = || Activity::Recorded {
            run: run(RunState::Running, 120_000),
            checked: None,
        };
        assert_eq!(latest(started(), 0), lines("Started 2m ago", None));
        assert_eq!(
            latest(started(), 2),
            lines("2 problems", Some("Started 2m ago"))
        );
        assert_eq!(
            latest(Activity::Running, 2),
            lines("2 problems", Some("Running"))
        );
    }

    #[test]
    fn a_routine_whose_runs_were_all_skipped_says_when_it_checked() {
        let at = || Activity::Checked(checked(MINUTES_4).unwrap());
        assert_eq!(latest(at(), 0), lines("Checked", Some("4m ago")));
        assert_eq!(latest(at(), 1), lines("1 problem", Some("Checked 4m ago")));
    }

    #[test]
    fn launchd_alone_knows_no_time() {
        let observed = || Activity::Observed(Exit::Code { code: 0 });
        assert_eq!(
            latest(observed(), 0),
            lines("Succeeded", Some("Time unavailable"))
        );
        assert_eq!(latest(observed(), 1), lines("1 problem", Some("Succeeded")));
        assert_eq!(
            latest(Activity::NoRecording, 0),
            lines("No recorded runs", None)
        );
        assert_eq!(latest(Activity::Unavailable, 0), lines("Unavailable", None));
    }
}
