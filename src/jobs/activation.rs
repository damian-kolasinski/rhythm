//! How a supervisor starts a job, described for people and agents.
use serde::{Deserialize, Serialize};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Activation {
    /// Restarted whenever it exits (`always`) or when its keep-alive conditions say so.
    Service { always: bool },
    /// Started when any trigger fires; no triggers means only on demand.
    Triggered { triggers: Vec<Trigger> },
}
impl Activation {
    pub fn is_service(&self) -> bool {
        matches!(self, Self::Service { .. })
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Trigger {
    AtLoad,
    Interval { seconds: u64 },
    Calendar(Calendar),
    PathChange,
    QueueDirectory,
    Socket,
    Mount,
}

/// A calendar match; an absent field matches every value, as in launchd.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Calendar {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub minute: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub hour: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub day: Option<u8>,
    /// 0 and 7 are Sunday.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weekday: Option<u8>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub month: Option<u8>,
}

impl fmt::Display for Activation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Service { always: true } => f.write_str("keep-alive"),
            Self::Service { always: false } => f.write_str("keep-alive (conditional)"),
            Self::Triggered { triggers } if triggers.is_empty() => f.write_str("on demand"),
            Self::Triggered { triggers } => {
                for (i, trigger) in triggers.iter().enumerate() {
                    if i > 0 {
                        f.write_str(", ")?;
                    }
                    write!(f, "{trigger}")?;
                }
                Ok(())
            }
        }
    }
}
impl fmt::Display for Trigger {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::AtLoad => f.write_str("at load"),
            Self::Interval { seconds } => {
                let (value, unit) = [(86400, "d"), (3600, "h"), (60, "m")]
                    .into_iter()
                    .find(|(size, _)| seconds % size == 0)
                    .map_or((*seconds, "s"), |(size, unit)| (seconds / size, unit));
                write!(f, "every {value}{unit}")
            }
            Self::Calendar(calendar) => write!(f, "{calendar}"),
            Self::PathChange => f.write_str("on path change"),
            Self::QueueDirectory => f.write_str("on queued file"),
            Self::Socket => f.write_str("on connection"),
            Self::Mount => f.write_str("on mount"),
        }
    }
}
impl fmt::Display for Calendar {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const DAYS: [&str; 8] = ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];
        let Self {
            minute,
            hour,
            day,
            weekday,
            month,
        } = *self;
        match (minute, hour, day, weekday, month) {
            (Some(m), Some(h), None, None, None) => write!(f, "daily {h:02}:{m:02}"),
            (Some(m), Some(h), None, Some(w @ 0..=7), None) => {
                write!(f, "{} {h:02}:{m:02}", DAYS[w as usize])
            }
            (Some(m), Some(h), Some(d), None, None) => write!(f, "monthly day {d} {h:02}:{m:02}"),
            (Some(m), None, None, None, None) => write!(f, "hourly at :{m:02}"),
            _ => {
                f.write_str("calendar")?;
                for (name, value) in [
                    ("minute", minute),
                    ("hour", hour),
                    ("day", day),
                    ("weekday", weekday),
                    ("month", month),
                ] {
                    if let Some(value) = value {
                        write!(f, " {name}={value}")?;
                    }
                }
                Ok(())
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn calendar(minute: Option<u8>, hour: Option<u8>, weekday: Option<u8>) -> Trigger {
        Trigger::Calendar(Calendar {
            minute,
            hour,
            weekday,
            ..Calendar::default()
        })
    }

    #[test]
    fn describes_common_schedules_compactly() {
        let cases = [
            (Activation::Service { always: true }, "keep-alive"),
            (
                Activation::Service { always: false },
                "keep-alive (conditional)",
            ),
            (Activation::Triggered { triggers: vec![] }, "on demand"),
            (
                Activation::Triggered {
                    triggers: vec![Trigger::AtLoad, calendar(Some(7), Some(8), None)],
                },
                "at load, daily 08:07",
            ),
            (
                Activation::Triggered {
                    triggers: vec![calendar(Some(0), Some(9), Some(6))],
                },
                "Sat 09:00",
            ),
            (
                Activation::Triggered {
                    triggers: vec![calendar(Some(0), Some(9), Some(7))],
                },
                "Sun 09:00",
            ),
            (
                Activation::Triggered {
                    triggers: vec![calendar(Some(30), None, None)],
                },
                "hourly at :30",
            ),
            (
                Activation::Triggered {
                    triggers: vec![calendar(None, Some(8), None)],
                },
                "calendar hour=8",
            ),
            (
                Activation::Triggered {
                    triggers: vec![Trigger::Interval { seconds: 300 }, Trigger::QueueDirectory],
                },
                "every 5m, on queued file",
            ),
        ];
        for (activation, expected) in cases {
            assert_eq!(activation.to_string(), expected);
        }
    }

    #[test]
    fn intervals_use_the_largest_exact_unit() {
        for (seconds, expected) in [
            (45, "every 45s"),
            (90, "every 90s"),
            (120, "every 2m"),
            (7200, "every 2h"),
            (172800, "every 2d"),
        ] {
            assert_eq!(Trigger::Interval { seconds }.to_string(), expected);
        }
    }
}
