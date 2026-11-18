use rhythm::{
    authoring::OWNED_PREFIX,
    jobs::{Activation, Exit, Health, Observation},
    runs::{Outcome, Run, RunState, Timestamp},
    status::JobReport,
    terminal_text::clean,
    time_text,
};

pub struct Routine {
    pub name: String,
    pub label: String,
    pub state: Health,
    pub trigger: String,
    pub activity: Activity,
}

pub enum Activity {
    Recorded(Run),
    Running,
    Observed(Exit),
    None,
}

impl From<JobReport> for Routine {
    fn from(job: JobReport) -> Self {
        let label = clean(&job.label);
        let short = label
            .strip_prefix(OWNED_PREFIX)
            .unwrap_or_else(|| label.rsplit('.').next().unwrap_or(&label));
        let name = short.replace(['-', '_'], " ");
        let name = match name.char_indices().nth(1) {
            Some((end, _)) => name[..end].to_uppercase() + &name[end..],
            None => name.to_uppercase(),
        };
        let trigger = match job.activation {
            Some(Activation::Service { always: true }) => "Keep alive".into(),
            Some(Activation::Service { always: false }) => "Conditional keep-alive".into(),
            Some(activation) => {
                let mut text = activation.to_string();
                text.get_mut(..1).map(str::make_ascii_uppercase);
                text
            }
            None => "Unavailable".into(),
        };
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
            _ => Activity::None,
        };
        Self {
            name,
            label,
            state: job.health,
            trigger,
            activity,
        }
    }
}

impl Activity {
    pub fn text(&self, now: Timestamp) -> (String, Option<String>) {
        match self {
            Self::None => ("No recorded runs".into(), None),
            Self::Running => ("Running".into(), None),
            Self::Observed(exit) => (
                if exit.succeeded() {
                    "Succeeded".into()
                } else {
                    exit.to_string()
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
                RunState::Finished { ended_at, outcome } => {
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
