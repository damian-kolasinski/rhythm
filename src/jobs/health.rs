//! What the supervisor last observed and whether that needs the operator.
use super::Activation;
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Observation {
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

/// Ordered by urgency: the first three need attention.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Health {
    /// The definition cannot be read, so the job cannot run as intended.
    Invalid,
    Failing,
    /// A keep-alive job that is not running.
    Down,
    Running,
    Ok,
    /// Loaded but has not finished a run yet.
    Idle,
    /// Defined but not loaded; possibly disabled on purpose.
    Unloaded,
}
impl Health {
    pub fn assess(activation: Option<&Activation>, observation: &Observation) -> Self {
        let Some(activation) = activation else {
            return Self::Invalid;
        };
        match *observation {
            Observation::Unloaded => Self::Unloaded,
            Observation::Loaded { pid: Some(_), .. } => Self::Running,
            Observation::Loaded { last_exit, .. } => match (activation, last_exit) {
                (_, Some(exit)) if !exit.succeeded() => Self::Failing,
                (Activation::Service { always: true }, _) => Self::Down,
                (_, Some(_)) => Self::Ok,
                (_, None) => Self::Idle,
            },
        }
    }
    pub fn needs_attention(self) -> bool {
        self <= Self::Down
    }
    pub fn name(self) -> &'static str {
        match self {
            Self::Invalid => "invalid",
            Self::Failing => "failing",
            Self::Down => "down",
            Self::Running => "running",
            Self::Ok => "ok",
            Self::Idle => "idle",
            Self::Unloaded => "unloaded",
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
                Health::assess(activation, &observation),
                expected,
                "{observation:?}"
            );
        }
    }

    #[test]
    fn only_broken_states_need_attention() {
        let attention: Vec<_> = [
            Health::Invalid,
            Health::Failing,
            Health::Down,
            Health::Running,
            Health::Ok,
            Health::Idle,
            Health::Unloaded,
        ]
        .into_iter()
        .filter(|h| h.needs_attention())
        .collect();
        assert_eq!(attention, [Health::Invalid, Health::Failing, Health::Down]);
    }
}
