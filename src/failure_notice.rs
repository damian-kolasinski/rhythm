//! Decide when a finished run is worth interrupting someone for: when a job
//! starts failing and when it recovers, never for every repeated failure.
use crate::{
    authoring::OWNED_PREFIX,
    runs::{Outcome, RunState, Verdict},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Failed(Outcome),
    Recovered,
}
impl Notice {
    /// `previous` is the job's last run before `run` that said whether the
    /// job works. A run that says nothing, as a skip or a requested stop, is
    /// neither a failure nor a recovery.
    pub fn after(previous: Option<&RunState>, run: &RunState) -> Option<Self> {
        let previously_failed = previous.is_some_and(RunState::failed);
        match (run, run.verdict()) {
            (RunState::Finished { outcome, .. }, Some(Verdict::Failed)) if !previously_failed => {
                Some(Self::Failed(*outcome))
            }
            (_, Some(Verdict::Succeeded)) if previously_failed => Some(Self::Recovered),
            _ => None,
        }
    }
    pub fn title(&self, label: &str) -> String {
        match self {
            Self::Failed(_) => format!("{label} failed"),
            Self::Recovered => format!("{label} recovered"),
        }
    }
    pub fn message(&self, label: &str) -> String {
        match self {
            Self::Failed(outcome) => {
                let short = label.strip_prefix(OWNED_PREFIX).unwrap_or(label);
                format!("{outcome}. See rhythm logs {short}")
            }
            Self::Recovered => "The latest run succeeded.".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::runs::{RunReport, Timestamp};

    fn ended(outcome: Outcome, skipped: bool) -> RunState {
        RunState::Finished {
            ended_at: Timestamp(0),
            outcome,
            report: RunReport {
                skipped,
                ..RunReport::default()
            },
        }
    }
    fn finished(code: i32) -> RunState {
        ended(Outcome::Code { code }, false)
    }

    #[test]
    fn notifies_on_transitions_only() {
        let failed = Outcome::Code { code: 1 };
        let ok = finished(0);
        assert_eq!(
            Notice::after(None, &finished(1)),
            Some(Notice::Failed(failed))
        );
        assert_eq!(Notice::after(None, &ok), None);
        assert_eq!(
            Notice::after(Some(&finished(0)), &finished(1)),
            Some(Notice::Failed(failed))
        );
        assert_eq!(Notice::after(Some(&finished(2)), &finished(1)), None);
        assert_eq!(Notice::after(Some(&RunState::Lost), &finished(1)), None);
        assert_eq!(
            Notice::after(Some(&finished(2)), &ok),
            Some(Notice::Recovered)
        );
        assert_eq!(
            Notice::after(Some(&RunState::Lost), &ok),
            Some(Notice::Recovered)
        );
        assert_eq!(Notice::after(Some(&finished(0)), &ok), None);
        let stopped = ended(Outcome::Stopped { signal: 15 }, false);
        assert_eq!(Notice::after(None, &stopped), None);
        assert_eq!(
            Notice::after(
                Some(&RunState::Running),
                &ended(Outcome::SpawnFailed, false)
            ),
            Some(Notice::Failed(Outcome::SpawnFailed))
        );
    }

    #[test]
    fn a_skip_is_neither_a_failure_nor_a_recovery() {
        let skip = ended(Outcome::Code { code: 0 }, true);
        assert_eq!(Notice::after(Some(&finished(1)), &skip), None);
        assert_eq!(Notice::after(None, &skip), None);
    }

    #[test]
    fn says_what_failed_and_where_to_look() {
        let notice = Notice::Failed(Outcome::Signal { signal: 9 });
        assert_eq!(
            notice.title("dev.rhythm.report"),
            "dev.rhythm.report failed"
        );
        assert_eq!(
            notice.message("dev.rhythm.report"),
            "signal 9. See rhythm logs report"
        );
        assert_eq!(
            notice.message("com.example.scan"),
            "signal 9. See rhythm logs com.example.scan"
        );
    }
}
