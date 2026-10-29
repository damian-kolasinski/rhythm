//! Parse the schedule flags of `rhythm add` into an activation.
use crate::{
    jobs::{Calendar, Schedule},
    operation_error::{Error, Result},
};

#[derive(Debug, Default)]
pub struct ScheduleInput<'a> {
    pub every: Option<&'a str>,
    pub daily: &'a [String],
    pub weekly: &'a [String],
    pub at_load: bool,
    pub keep_alive: bool,
}

/// No flags means the job only runs when started with `rhythm kick`.
pub fn schedule(input: &ScheduleInput) -> Result<Schedule> {
    if input.keep_alive {
        let alone = input.every.is_none()
            && input.daily.is_empty()
            && input.weekly.is_empty()
            && !input.at_load;
        return alone
            .then_some(Schedule::KeepAlive)
            .ok_or(Error::InvalidRequest);
    }
    let mut calendar = vec![];
    for time in input.daily {
        let (hour, minute) = clock(time)?;
        calendar.push(entry(hour, minute, None));
    }
    for weekly in input.weekly {
        let (day, time) = weekly.split_once('@').ok_or(Error::InvalidRequest)?;
        let (hour, minute) = clock(time)?;
        calendar.push(entry(hour, minute, Some(weekday(day)?)));
    }
    Ok(Schedule::Triggered {
        at_load: input.at_load,
        every_seconds: input.every.map(interval).transpose()?,
        calendar,
    })
}

/// As in `90s`, `5m`, `2h`, or `1d`, up to a year.
pub fn interval(text: &str) -> Result<u64> {
    let (number, unit) = [("s", 1), ("m", 60), ("h", 3600), ("d", 86400)]
        .into_iter()
        .find_map(|(suffix, unit)| text.strip_suffix(suffix).map(|n| (n, unit)))
        .ok_or(Error::InvalidRequest)?;
    if number.is_empty() || !number.bytes().all(|b| b.is_ascii_digit()) {
        return Err(Error::InvalidRequest);
    }
    number
        .parse::<u64>()
        .ok()
        .filter(|n| *n > 0)
        .and_then(|n| n.checked_mul(unit))
        .filter(|s| *s <= 365 * 86400)
        .ok_or(Error::InvalidRequest)
}

/// 24-hour `HH:MM` in the machine's local time, as launchd interprets it.
fn clock(text: &str) -> Result<(u8, u8)> {
    let (hour, minute) = text.split_once(':').ok_or(Error::InvalidRequest)?;
    let field = |value: &str, max: u8| {
        value
            .parse::<u8>()
            .ok()
            .filter(|v| {
                *v <= max
                    && (1..=2).contains(&value.len())
                    && value.bytes().all(|b| b.is_ascii_digit())
            })
            .ok_or(Error::InvalidRequest)
    };
    if minute.len() != 2 {
        return Err(Error::InvalidRequest);
    }
    Ok((field(hour, 23)?, field(minute, 59)?))
}

/// A three-letter or full English day name, in any case.
fn weekday(text: &str) -> Result<u8> {
    const DAYS: [&str; 7] = [
        "sunday",
        "monday",
        "tuesday",
        "wednesday",
        "thursday",
        "friday",
        "saturday",
    ];
    let text = text.to_ascii_lowercase();
    DAYS.iter()
        .position(|day| text == *day || text == day[..3])
        .map(|day| day as u8)
        .ok_or(Error::InvalidRequest)
}

fn entry(hour: u8, minute: u8, weekday: Option<u8>) -> Calendar {
    Calendar {
        hour: Some(hour),
        minute: Some(minute),
        weekday,
        ..Calendar::default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::jobs::Activation;

    fn describe(input: ScheduleInput) -> Result<String> {
        schedule(&input).map(|s| Activation::from(&s).to_string())
    }

    fn activation(input: &ScheduleInput) -> Result<Schedule> {
        schedule(input)
    }

    #[test]
    fn combines_triggers_and_describes_them() {
        let daily = ["08:07".to_owned(), "18:30".to_owned()];
        let weekly = ["sat@9:00".to_owned(), "Monday@07:45".to_owned()];
        assert_eq!(
            describe(ScheduleInput {
                every: Some("5m"),
                daily: &daily,
                weekly: &weekly,
                at_load: true,
                keep_alive: false,
            }),
            Ok("at load, every 5m, daily 08:07, daily 18:30, Sat 09:00, Mon 07:45".into())
        );
        assert_eq!(describe(ScheduleInput::default()), Ok("on demand".into()));
        assert_eq!(
            describe(ScheduleInput {
                keep_alive: true,
                ..Default::default()
            }),
            Ok("keep-alive".into())
        );
    }

    #[test]
    fn refuses_ambiguous_or_malformed_schedules() {
        let bad_daily = [
            "8", "24:00", "08:60", "08:7", "8:07pm", "-1:00", "+8:00", "008:00",
        ];
        for time in bad_daily {
            let daily = [time.to_owned()];
            assert!(
                activation(&ScheduleInput {
                    daily: &daily,
                    ..Default::default()
                })
                .is_err(),
                "{time}"
            );
        }
        for entry in ["sat", "funday@09:00", "sa@09:00", "satur@09:00", "@09:00"] {
            let weekly = [entry.to_owned()];
            assert!(
                activation(&ScheduleInput {
                    weekly: &weekly,
                    ..Default::default()
                })
                .is_err(),
                "{entry}"
            );
        }
        assert!(
            activation(&ScheduleInput {
                keep_alive: true,
                at_load: true,
                ..Default::default()
            })
            .is_err()
        );
    }

    #[test]
    fn parses_intervals_within_a_year() {
        for (text, seconds) in [("90s", 90), ("5m", 300), ("2h", 7200), ("1d", 86400)] {
            assert_eq!(interval(text), Ok(seconds));
        }
        for text in [
            "", "m", "0m", "5", "5w", "-5m", "+5m", "366d", "5 m", "1.5h",
        ] {
            assert!(interval(text).is_err(), "{text:?}");
        }
    }
}
