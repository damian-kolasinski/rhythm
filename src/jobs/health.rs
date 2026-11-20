//! What the supervisor last observed and whether that needs the operator.
use super::Activation;
use crate::runs::Verdict;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Observation {
    Unavailable,
    Unloaded,
    Loaded {
        #[serde(default, skip_serializing_if = "Option::is_none")]
        pid: Option<u32>,
        runs: u64,
        /// `None` until the job has exited at least once.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        last_exit: Option<Exit>,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Exit {
    Code {
        code: i32,
    },
    Signal {
        signal: i32,
    },
    /// launchd stopped an idle job that opted into idle exit; not a failure.
    Reclaimed,
    /// The kernel killed the job under memory pressure or for exceeding its limit.
    Jetsam,
}
impl Exit {
    pub fn succeeded(self) -> bool {
        matches!(self, Self::Code { code: 0 } | Self::Reclaimed)
    }
}
impl fmt::Display for Exit {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Code { code } => write!(f, "exit {code}"),
            Self::Signal { signal } => write!(f, "signal {signal}"),
            Self::Reclaimed => f.write_str("idle exit"),
            Self::Jetsam => f.write_str("killed for memory"),
        }
    }
}

/// Ordered by urgency: the first four need attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// The definition cannot be read, so the job cannot run as intended.
    Invalid,
    Failing,
    /// A keep-alive job that is not running.
    Down,
    /// The job works but reports open problems. It ranks above running, so
    /// a long run never hides them; a broken job is that first.
    Flagged,
    Running,
    Ok,
    /// Loaded but has not finished a run yet.
    Idle,
    /// Defined but not loaded; possibly disabled on purpose.
    Unloaded,
    /// No observation is available from the job's scheduler, or another
    /// machine reported a state this version does not know.
    #[serde(other)]
    Unknown,
}
impl Health {
    /// `verdict` is what the newest recorded run that says whether the job
    /// works said. Recorded runs know about skips and requested stops, so the
    /// verdict decides over launchd's last exit; without one, launchd's
    /// observation does. `open_findings` is how many problems the job's
    /// newest statement left open.
    pub fn assess(
        activation: Option<&Activation>,
        observation: &Observation,
        verdict: Option<Verdict>,
        open_findings: usize,
    ) -> Self {
        let Some(activation) = activation else {
            return Self::Invalid;
        };
        match Self::observed(activation, observation, verdict) {
            health if health.broken() => health,
            _ if open_findings > 0 => Self::Flagged,
            health => health,
        }
    }
    fn observed(
        activation: &Activation,
        observation: &Observation,
        verdict: Option<Verdict>,
    ) -> Self {
        match *observation {
            Observation::Unavailable => Self::Unknown,
            Observation::Unloaded => Self::Unloaded,
            Observation::Loaded { pid: Some(_), .. } => Self::Running,
            Observation::Loaded { last_exit, .. } => {
                let verdict = verdict.or(last_exit.map(|exit| {
                    if exit.succeeded() {
                        Verdict::Succeeded
                    } else {
                        Verdict::Failed
                    }
                }));
                match (activation, verdict) {
                    (_, Some(Verdict::Failed)) => Self::Failing,
                    (Activation::Service { always: true }, _) => Self::Down,
                    (_, Some(Verdict::Succeeded)) => Self::Ok,
                    (_, None) => Self::Idle,
                }
            }
        }
    }
    /// Invalid, failing, down, or flagged.
    pub fn needs_attention(self) -> bool {
        self <= Self::Flagged
    }
    /// The job cannot do its work: invalid, failing, or down.
    pub fn broken(self) -> bool {
        self <= Self::Down
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Failing => "failing",
            Self::Down => "down",
            Self::Flagged => "flagged",
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Idle => "idle",
            Self::Unloaded => "unloaded",
            Self::Unknown => "unknown",
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn loaded(pid: Option<u32>, last_exit: Option<Exit>) -> Observation {
        Observation::Loaded {
            pid,
            runs: 1,
            last_exit,
        }
    }

    #[test]
    fn classifies_observations_by_activation() {
        let service = Activation::Service { always: true };
        let conditional = Activation::Service { always: false };
        let scheduled = Activation::Triggered { triggers: vec![] };
        let ok = Some(Exit::Code { code: 0 });
        let failed = Some(Exit::Code { code: 1 });
        let killed = Some(Exit::Signal { signal: 9 });
        let reclaimed = Some(Exit::Reclaimed);
        let jetsam = Some(Exit::Jetsam);
        let cases = [
            (None, loaded(Some(1), ok), Health::Invalid),
            (Some(&scheduled), Observation::Unloaded, Health::Unloaded),
            (Some(&service), loaded(Some(1), killed), Health::Running),
            (Some(&service), loaded(None, None), Health::Down),
            (Some(&service), loaded(None, ok), Health::Down),
            (Some(&service), loaded(None, killed), Health::Failing),
            (Some(&conditional), loaded(None, ok), Health::Ok),
            (Some(&conditional), loaded(None, failed), Health::Failing),
            (Some(&scheduled), loaded(None, None), Health::Idle),
            (Some(&scheduled), loaded(None, ok), Health::Ok),
            (Some(&scheduled), loaded(None, failed), Health::Failing),
            (Some(&scheduled), loaded(None, reclaimed), Health::Ok),
            (Some(&scheduled), loaded(None, jetsam), Health::Failing),
        ];
        for (activation, observation, expected) in cases {
            assert_eq!(
                Health::assess(activation, &observation, None, 0),
                expected,
                "{observation:?}"
            );
        }
    }

    #[test]
    fn a_recorded_verdict_decides_over_launchds_last_exit() {
        let scheduled = Activation::Triggered { triggers: vec![] };
        let service = Activation::Service { always: true };
        let ok = Some(Exit::Code { code: 0 });
        let stopped = Some(Exit::Signal { signal: 15 });
        let cases = [
            // A skip after a failed run exits 0, yet the job still fails.
            (
                &scheduled,
                loaded(None, ok),
                Some(Verdict::Failed),
                Health::Failing,
            ),
            // A stop request after a good run is a signal to launchd only.
            (
                &scheduled,
                loaded(None, stopped),
                Some(Verdict::Succeeded),
                Health::Ok,
            ),
            (
                &scheduled,
                loaded(None, None),
                Some(Verdict::Succeeded),
                Health::Ok,
            ),
            (
                &scheduled,
                loaded(Some(1), ok),
                Some(Verdict::Failed),
                Health::Running,
            ),
            (
                &service,
                loaded(None, ok),
                Some(Verdict::Succeeded),
                Health::Down,
            ),
            (
                &service,
                loaded(None, ok),
                Some(Verdict::Failed),
                Health::Failing,
            ),
            (
                &scheduled,
                Observation::Unloaded,
                Some(Verdict::Failed),
                Health::Unloaded,
            ),
        ];
        for (activation, observation, verdict, expected) in cases {
            assert_eq!(
                Health::assess(Some(activation), &observation, verdict, 0),
                expected,
                "{observation:?} {verdict:?}"
            );
        }
    }

    #[test]
    fn open_findings_flag_a_job_that_is_not_broken() {
        let scheduled = Activation::Triggered { triggers: vec![] };
        let service = Activation::Service { always: true };
        let ok = Some(Exit::Code { code: 0 });
        let failed = Some(Exit::Code { code: 1 });
        let cases = [
            (Some(&scheduled), loaded(None, ok), Health::Flagged),
            (Some(&scheduled), loaded(Some(1), ok), Health::Flagged),
            (Some(&scheduled), loaded(None, None), Health::Flagged),
            (Some(&scheduled), Observation::Unloaded, Health::Flagged),
            (Some(&scheduled), Observation::Unavailable, Health::Flagged),
            (Some(&scheduled), loaded(None, failed), Health::Failing),
            (Some(&service), loaded(None, ok), Health::Down),
            (None, loaded(None, ok), Health::Invalid),
        ];
        for (activation, observation, expected) in cases {
            assert_eq!(
                Health::assess(activation, &observation, None, 2),
                expected,
                "{observation:?}"
            );
        }
    }

    #[test]
    fn a_state_from_a_newer_rhythm_reads_as_unknown() {
        let health: Health = serde_json::from_value(serde_json::json!("from_the_future")).unwrap();
        assert_eq!(health, Health::Unknown);
        let flagged: Health = serde_json::from_value(serde_json::json!("flagged")).unwrap();
        assert_eq!(flagged, Health::Flagged);
    }

    #[test]
    fn broken_and_flagged_states_need_attention() {
        let all = [
            Health::Invalid,
            Health::Failing,
            Health::Down,
            Health::Flagged,
            Health::Running,
            Health::Ok,
            Health::Idle,
            Health::Unloaded,
            Health::Unknown,
        ];
        let attention: Vec<_> = all.into_iter().filter(|h| h.needs_attention()).collect();
        assert_eq!(
            attention,
            [
                Health::Invalid,
                Health::Failing,
                Health::Down,
                Health::Flagged
            ]
        );
        let broken: Vec<_> = all.into_iter().filter(|h| h.broken()).collect();
        assert_eq!(broken, [Health::Invalid, Health::Failing, Health::Down]);
    }
}
