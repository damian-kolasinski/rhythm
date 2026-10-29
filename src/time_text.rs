//! Short human renderings of instants and durations for terminal output.
use crate::runs::Timestamp;
use chrono::TimeZone;

pub fn ago(then: Timestamp, now: Timestamp) -> String {
    let ms = now.0 - then.0;
    if ms < 1000 {
        "just now".into()
    } else {
        format!("{} ago", coarse(ms))
    }
}

/// At most two units, as in `850ms`, `3.2s`, `4m 10s`, `2h 5m`, `3d 4h`.
pub fn duration(ms: i64) -> String {
    let ms = ms.max(0);
    match ms {
        0..1000 => format!("{ms}ms"),
        1000..60_000 => format!("{:.1}s", ms as f64 / 1000.0),
        _ => {
            let s = ms / 1000;
            let (big, big_unit, small, small_unit) = match s {
                0..3600 => (s / 60, "m", s % 60, "s"),
                3600..86400 => (s / 3600, "h", s % 3600 / 60, "m"),
                _ => (s / 86400, "d", s % 86400 / 3600, "h"),
            };
            if small == 0 {
                format!("{big}{big_unit}")
            } else {
                format!("{big}{big_unit} {small}{small_unit}")
            }
        }
    }
}

fn coarse(ms: i64) -> String {
    let s = ms / 1000;
    match s {
        0..60 => format!("{s}s"),
        60..3600 => format!("{}m", s / 60),
        3600..86400 => format!("{}h", s / 3600),
        _ => format!("{}d", s / 86400),
    }
}

pub fn local<Tz: TimeZone>(at: Timestamp, zone: &Tz) -> String
where
    Tz::Offset: std::fmt::Display,
{
    at.utc()
        .with_timezone(zone)
        .format("%Y-%m-%d %H:%M:%S")
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn renders_relative_times_coarsely() {
        let now = Timestamp(10_000_000_000);
        for (ago_ms, expected) in [
            (0, "just now"),
            (999, "just now"),
            (12_000, "12s ago"),
            (5 * 60_000 + 59_000, "5m ago"),
            (3 * 3_600_000, "3h ago"),
            (49 * 3_600_000, "2d ago"),
        ] {
            assert_eq!(ago(Timestamp(now.0 - ago_ms), now), expected);
        }
    }

    #[test]
    fn renders_durations_with_two_units_at_most() {
        for (ms, expected) in [
            (-5, "0ms"),
            (850, "850ms"),
            (3_240, "3.2s"),
            (60_000, "1m"),
            (250_000, "4m 10s"),
            (7_500_000, "2h 5m"),
            (273_600_000, "3d 4h"),
        ] {
            assert_eq!(duration(ms), expected);
        }
    }

    #[test]
    fn renders_local_wall_time() {
        let zone = chrono::FixedOffset::east_opt(3600).unwrap();
        assert_eq!(local(Timestamp(0), &zone), "1970-01-01 01:00:00");
    }
}
