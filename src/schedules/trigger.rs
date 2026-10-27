//! Pure recurrence calculation. Calendar gaps are skipped; folds use the earlier instant once.
use chrono::{DateTime, Datelike, Days, LocalResult, NaiveDate, TimeZone, Utc};
use chrono_tz::Tz;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "Input", into = "Input")]
pub struct Trigger(Input);
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case", deny_unknown_fields)]
enum Input {
    Interval {
        every_ms: i64,
    },
    Daily {
        hour: u32,
        minute: u32,
        timezone: String,
    },
    Weekly {
        weekdays: Vec<u32>,
        hour: u32,
        minute: u32,
        timezone: String,
    },
}
impl TryFrom<Input> for Trigger {
    type Error = &'static str;
    fn try_from(v: Input) -> Result<Self, Self::Error> {
        match &v {
            Input::Interval { every_ms } if !(100..=31_536_000_000).contains(every_ms) => {
                return Err("invalid_interval");
            }
            Input::Daily {
                hour,
                minute,
                timezone,
            }
            | Input::Weekly {
                hour,
                minute,
                timezone,
                ..
            } if *hour > 23 || *minute > 59 || timezone.parse::<Tz>().is_err() => {
                return Err("invalid_calendar");
            }
            _ => {}
        }
        if let Input::Weekly { weekdays, .. } = &v {
            let mut distinct = weekdays.clone();
            distinct.sort();
            distinct.dedup();
            if weekdays.is_empty()
                || weekdays.len() > 7
                || distinct.len() != weekdays.len()
                || weekdays.iter().any(|d| !(1..=7).contains(d))
            {
                return Err("invalid_weekdays");
            }
        }
        Ok(Self(v))
    }
}
impl From<Trigger> for Input {
    fn from(v: Trigger) -> Self {
        v.0
    }
}
impl Trigger {
    /// First occurrence strictly after the supplied timestamp.
    pub fn next_after(&self, after: i64) -> crate::operation_error::Result<i64> {
        match &self.0 {
            Input::Interval { every_ms } => after
                .checked_add(*every_ms)
                .ok_or(crate::operation_error::Error::InvalidRequest),
            _ => self.calendar_near(after, true),
        }
    }
    /// Latest due occurrence and next future occurrence, preserving interval phase.
    pub fn due_window(&self, first: i64, now: i64) -> crate::operation_error::Result<(i64, i64)> {
        debug_assert!(first <= now);
        match &self.0 {
            Input::Interval { every_ms } => {
                let elapsed = now
                    .checked_sub(first)
                    .ok_or(crate::operation_error::Error::InvalidRequest)?;
                let latest = first + elapsed / every_ms * every_ms;
                Ok((latest, self.next_after(latest)?))
            }
            _ => Ok((
                self.calendar_near(now, false)?.max(first),
                self.calendar_near(now, true)?,
            )),
        }
    }
    fn calendar_near(&self, at: i64, forward: bool) -> crate::operation_error::Result<i64> {
        let (hour, minute, timezone, weekdays) = match &self.0 {
            Input::Daily {
                hour,
                minute,
                timezone,
            } => (*hour, *minute, timezone, None),
            Input::Weekly {
                hour,
                minute,
                timezone,
                weekdays,
            } => (*hour, *minute, timezone, Some(weekdays)),
            _ => unreachable!(),
        };
        let zone: Tz = timezone.parse().unwrap();
        let date = DateTime::<Utc>::from_timestamp_millis(at)
            .ok_or(crate::operation_error::Error::InvalidRequest)?
            .with_timezone(&zone)
            .date_naive();
        // Two weeks includes a skipped weekly occurrence at a DST gap.
        for offset in 0..=15 {
            let day = if forward {
                date.checked_add_days(Days::new(offset))
            } else {
                date.checked_sub_days(Days::new(offset))
            }
            .ok_or(crate::operation_error::Error::InvalidRequest)?;
            if weekdays.is_some_and(|days| !days.contains(&day.weekday().number_from_monday())) {
                continue;
            }
            if let Some(value) = instant(zone, day, hour, minute)
                && if forward { value > at } else { value <= at }
            {
                return Ok(value);
            }
        }
        Err(crate::operation_error::Error::InvalidRequest)
    }
}
fn instant(zone: Tz, day: NaiveDate, hour: u32, minute: u32) -> Option<i64> {
    match zone.from_local_datetime(&day.and_hms_opt(hour, minute, 0).unwrap()) {
        LocalResult::None => None,
        LocalResult::Single(t) | LocalResult::Ambiguous(t, _) => Some(t.timestamp_millis()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    fn ms(s: &str) -> i64 {
        DateTime::parse_from_rfc3339(s).unwrap().timestamp_millis()
    }
    #[test]
    fn interval_skips_large_gaps_without_losing_phase() {
        let t: Trigger = serde_json::from_value(json!({"type":"interval","every_ms":100})).unwrap();
        assert_eq!(t.next_after(1000).unwrap(), 1100);
        assert_eq!(
            t.due_window(1100, 1_000_000_099).unwrap(),
            (1_000_000_000, 1_000_000_100)
        );
    }
    #[test]
    fn daily_skips_spring_gap_and_runs_fall_fold_once_at_earlier_instant() {
        let t: Trigger = serde_json::from_value(
            json!({"type":"daily","hour":2,"minute":30,"timezone":"Europe/Berlin"}),
        )
        .unwrap();
        assert_eq!(
            t.next_after(ms("2026-03-28T01:30:00Z")).unwrap(),
            ms("2026-03-30T00:30:00Z")
        );
        let first = ms("2026-10-25T00:30:00Z");
        assert_eq!(t.next_after(first - 1).unwrap(), first);
        assert_eq!(t.next_after(first).unwrap(), ms("2026-10-26T01:30:00Z"));
        assert_eq!(
            t.due_window(first, ms("2026-10-25T01:45:00Z")).unwrap().0,
            first
        );
        let weekly: Trigger = serde_json::from_value(
            json!({"type":"weekly","weekdays":[7],"hour":2,"minute":30,"timezone":"Europe/Berlin"}),
        )
        .unwrap();
        assert_eq!(
            weekly.next_after(ms("2026-03-22T01:30:00Z")).unwrap(),
            ms("2026-04-05T00:30:00Z")
        );
    }
    #[test]
    fn weekly_checks_weekdays_and_leap_day() {
        let t: Trigger = serde_json::from_value(
            json!({"type":"weekly","weekdays":[4],"hour":9,"minute":15,"timezone":"UTC"}),
        )
        .unwrap();
        assert_eq!(
            t.next_after(ms("2024-02-28T12:00:00Z")).unwrap(),
            ms("2024-02-29T09:15:00Z")
        );
        assert_eq!(
            t.next_after(ms("2024-02-29T09:15:00Z")).unwrap(),
            ms("2024-03-07T09:15:00Z")
        );
        let multiple: Trigger = serde_json::from_value(
            json!({"type":"weekly","weekdays":[1,5],"hour":9,"minute":15,"timezone":"UTC"}),
        )
        .unwrap();
        assert_eq!(
            multiple.next_after(ms("2024-02-29T12:00:00Z")).unwrap(),
            ms("2024-03-01T09:15:00Z")
        );
    }
    #[test]
    fn rejects_invalid_boundaries_unknown_zones_and_extra_fields() {
        for v in [
            json!({"type":"interval","every_ms":99}),
            json!({"type":"interval","every_ms":31_536_000_001_i64}),
            json!({"type":"daily","hour":24,"minute":0,"timezone":"UTC"}),
            json!({"type":"daily","hour":0,"minute":60,"timezone":"UTC"}),
            json!({"type":"daily","hour":0,"minute":0,"timezone":"Unknown/Zone"}),
            json!({"type":"weekly","hour":0,"minute":0,"timezone":"UTC","weekdays":[]}),
            json!({"type":"weekly","hour":0,"minute":0,"timezone":"UTC","weekdays":[1,1]}),
            json!({"type":"weekly","hour":0,"minute":0,"timezone":"UTC","weekdays":[8]}),
            json!({"type":"interval","every_ms":100,"secret":"synthetic"}),
        ] {
            assert!(serde_json::from_value::<Trigger>(v).is_err());
        }
    }
}
