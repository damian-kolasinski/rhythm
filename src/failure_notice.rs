//! Decide when a finished run is worth interrupting someone for: when a job
//! starts failing and when it recovers, never for every repeated failure.
use crate::{
    authoring::OWNED_PREFIX,
    runs::{Outcome, RunState},
};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Notice {
    Failed(Outcome),
    Recovered,
}
impl Notice {
    /// `previous` is the job's run before this one, if it had one.
    pub fn after(previous: Option<&RunState>, outcome: Outcome) -> Option<Self> {
        let previously_failed = previous.is_some_and(RunState::failed);
        match (outcome.succeeded(), outcome.failed(), previously_failed) {
            (_, true, false) => Some(Self::Failed(outcome)),
            (true, _, true) => Some(Self::Recovered),
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
    use crate::runs::Timestamp;

    fn finished(code: i32) -> RunState {
        RunState::Finished {
            ended_at: Timestamp(0),
            outcome: Outcome::Code { code },
        }
    }

    #[test]
    fn notifies_on_transitions_only() {
        let failed = Outcome::Code { code: 1 };
        let ok = Outcome::Code { code: 0 };
        assert_eq!(Notice::after(None, failed), Some(Notice::Failed(failed)));
        assert_eq!(Notice::after(None, ok), None);
        assert_eq!(
            Notice::after(Some(&finished(0)), failed),
            Some(Notice::Failed(failed))
        );
        assert_eq!(Notice::after(Some(&finished(2)), failed), None);
        assert_eq!(Notice::after(Some(&RunState::Lost), failed), None);
        assert_eq!(
            Notice::after(Some(&finished(2)), ok),
            Some(Notice::Recovered)
        );
        assert_eq!(
            Notice::after(Some(&RunState::Lost), ok),
            Some(Notice::Recovered)
        );
        assert_eq!(Notice::after(Some(&finished(0)), ok), None);
        assert_eq!(Notice::after(None, Outcome::Stopped { signal: 15 }), None);
        assert_eq!(
            Notice::after(Some(&RunState::Running), Outcome::SpawnFailed),
            Some(Notice::Failed(Outcome::SpawnFailed))
        );
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
