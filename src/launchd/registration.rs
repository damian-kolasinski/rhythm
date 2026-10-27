use crate::discovery::{
    Calendar, DiagnosticCode, ObservedState, Registration, Requirement, Trigger, WorkKind,
};
use plist::{Dictionary, Value};
use std::path::Path;
use std::{
    io::{Cursor, Read},
    os::unix::fs::OpenOptionsExt,
};

type Result<T> = std::result::Result<T, DiagnosticCode>;

pub(super) fn read(path: &Path) -> Result<Registration> {
    // Open nonblocking before checking the descriptor, avoiding FIFO and symlink races.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(libc::O_NONBLOCK)
        .open(path)
        .map_err(|_| DiagnosticCode::EntryUnreadable)?;
    let metadata = file
        .metadata()
        .map_err(|_| DiagnosticCode::EntryUnreadable)?;
    const MAX_BYTES: u64 = 1024 * 1024;
    if !metadata.is_file() || metadata.len() > MAX_BYTES {
        return Err(DiagnosticCode::UnsupportedRegistration);
    }
    let mut bytes = Vec::new();
    file.take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| DiagnosticCode::EntryUnreadable)?;
    if bytes.len() as u64 > MAX_BYTES {
        return Err(DiagnosticCode::UnsupportedRegistration);
    }
    let value =
        Value::from_reader(Cursor::new(bytes)).map_err(|_| DiagnosticCode::InvalidRegistration)?;
    decode(value, path)
}

fn invalid<T>() -> Result<T> {
    Err(DiagnosticCode::InvalidRegistration)
}

fn decode(value: Value, path: &Path) -> Result<Registration> {
    let d = value
        .as_dictionary()
        .ok_or(DiagnosticCode::InvalidRegistration)?;
    let label = d
        .get("Label")
        .and_then(Value::as_string)
        .filter(|s| {
            !s.is_empty() && !s.chars().any(char::is_whitespace) && !s.chars().any(char::is_control)
        })
        .ok_or(DiagnosticCode::InvalidRegistration)?
        .to_owned();
    let program = optional_string(d, "Program")?;
    let args = match d.get("ProgramArguments") {
        None => None,
        Some(Value::Array(args))
            if !args.is_empty()
                && args
                    .iter()
                    .all(|a| a.as_string().is_some_and(|s| !s.contains('\0'))) =>
        {
            if args[0].as_string().is_none_or(str::is_empty) {
                return invalid();
            }
            Some(args)
        }
        _ => return invalid(),
    };
    if program.is_none() && args.is_none() {
        return invalid();
    }
    let mut requirements = vec![Requirement::VerifyWorkloadCompletion];
    let mut service = false;
    match d.get("KeepAlive") {
        None | Some(Value::Boolean(false)) => {}
        Some(Value::Boolean(true)) => {
            service = true;
            requirements.push(Requirement::RestartAlways);
        }
        Some(Value::Dictionary(_)) => {
            service = true;
            requirements.push(Requirement::ConditionalRestart);
        }
        _ => return invalid(),
    }
    let mut triggers = Vec::new();
    if let Some(value) = d.get("StartInterval") {
        let seconds = value
            .as_unsigned_integer()
            .filter(|s| *s > 0)
            .ok_or(DiagnosticCode::InvalidRegistration)?;
        triggers.push(Trigger::Interval { seconds });
        requirements.push(Requirement::IntervalScheduling);
    }
    if let Some(value) = d.get("StartCalendarInterval") {
        let calendars = match value {
            Value::Dictionary(c) => vec![c],
            Value::Array(cs) if !cs.is_empty() => cs
                .iter()
                .map(|c| c.as_dictionary().ok_or(DiagnosticCode::InvalidRegistration))
                .collect::<Result<Vec<_>>>()?,
            _ => return invalid(),
        };
        for c in calendars {
            if c.keys()
                .any(|k| !["Month", "Day", "Weekday", "Hour", "Minute"].contains(&k.as_str()))
            {
                return Err(DiagnosticCode::UnsupportedRegistration);
            }
            triggers.push(Trigger::Calendar {
                components: Calendar {
                    month: component(c, "Month", 1, 12)?,
                    day: component(c, "Day", 1, 31)?,
                    weekday: component(c, "Weekday", 0, 7)?,
                    hour: component(c, "Hour", 0, 23)?,
                    minute: component(c, "Minute", 0, 59)?,
                },
            });
        }
        requirements.push(Requirement::CalendarScheduling);
    }
    let run_at_load = match d.get("RunAtLoad") {
        None => false,
        Some(value) => value
            .as_boolean()
            .ok_or(DiagnosticCode::InvalidRegistration)?,
    };
    if service || run_at_load {
        triggers.push(Trigger::AtLoad);
        requirements.push(Requirement::StartAtLoad);
    }
    let environment_variable_count = match d.get("EnvironmentVariables") {
        None => 0,
        Some(Value::Dictionary(env))
            if env
                .values()
                .all(|v| v.as_string().is_some_and(|s| !s.contains('\0'))) =>
        {
            env.len()
        }
        _ => return invalid(),
    };
    if environment_variable_count > 0 {
        requirements.push(Requirement::EnvironmentConfiguration);
    }
    if optional_string(d, "WorkingDirectory")?.is_some() {
        requirements.push(Requirement::WorkingDirectory);
    }
    if optional_string(d, "StandardOutPath")?.is_some()
        | optional_string(d, "StandardErrorPath")?.is_some()
    {
        requirements.push(Requirement::OutputRouting);
    }
    if let Some(v) = d.get("ThrottleInterval") {
        if v.as_unsigned_integer().is_none() {
            return invalid();
        }
        requirements.push(Requirement::RestartThrottle);
    }
    if optional_string(d, "ProcessType")?.is_some() {
        requirements.push(Requirement::ExecutionContext);
    }
    // Preserve visibility of provider features we cannot safely translate yet.
    let understood = [
        "Label",
        "Program",
        "ProgramArguments",
        "KeepAlive",
        "StartInterval",
        "StartCalendarInterval",
        "RunAtLoad",
        "EnvironmentVariables",
        "WorkingDirectory",
        "StandardOutPath",
        "StandardErrorPath",
        "ThrottleInterval",
        "ProcessType",
    ];
    if d.keys().any(|key| !understood.contains(&key.as_str())) {
        requirements.push(Requirement::AdditionalLaunchdBehavior);
    }
    let kind = if service {
        WorkKind::Service
    } else if triggers.iter().any(|t| !matches!(t, Trigger::AtLoad)) {
        WorkKind::Schedule
    } else {
        WorkKind::OnDemand
    };
    requirements.sort();
    requirements.dedup();
    Ok(Registration {
        label,
        source: path.to_owned(),
        kind,
        observed: ObservedState::Unknown,
        triggers,
        environment_variable_count,
        requirements,
    })
}

fn optional_string<'a>(d: &'a Dictionary, key: &str) -> Result<Option<&'a str>> {
    match d.get(key) {
        None => Ok(None),
        Some(Value::String(s)) if !s.is_empty() && !s.contains('\0') => Ok(Some(s)),
        _ => invalid(),
    }
}

fn component(d: &Dictionary, key: &str, min: u8, max: u8) -> Result<Option<u8>> {
    match d.get(key) {
        None => Ok(None),
        Some(v) => v
            .as_unsigned_integer()
            .and_then(|n| u8::try_from(n).ok())
            .filter(|n| (min..=max).contains(n))
            .map(Some)
            .ok_or(DiagnosticCode::InvalidRegistration),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn registration(extra: &str) -> Result<Registration> {
        let xml = format!(
            "<plist version=\"1.0\"><dict><key>Label</key><string>sample</string><key>Program</key><string>/bin/true</string>{extra}</dict></plist>"
        );
        decode(
            Value::from_reader(std::io::Cursor::new(xml)).unwrap(),
            Path::new("sample.plist"),
        )
    }

    #[test]
    fn keepalive_implies_start_at_load_without_an_explicit_flag() {
        for extra in [
            "<key>KeepAlive</key><true/>",
            "<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>",
            "<key>KeepAlive</key><true/><key>RunAtLoad</key><true/>",
        ] {
            let r = registration(extra).unwrap();
            assert_eq!(r.triggers, vec![Trigger::AtLoad]);
            assert!(r.requirements.contains(&Requirement::StartAtLoad));
        }
        let r = registration("<key>KeepAlive</key><false/>").unwrap();
        assert!(r.triggers.is_empty());
    }

    #[test]
    fn calendar_components_accept_all_documented_boundaries() {
        for (key, min, max) in [
            ("Month", 1, 12),
            ("Day", 1, 31),
            ("Weekday", 0, 7),
            ("Hour", 0, 23),
            ("Minute", 0, 59),
        ] {
            for n in [min, max] {
                assert!(registration(&format!("<key>StartCalendarInterval</key><dict><key>{key}</key><integer>{n}</integer></dict>")).is_ok());
            }
            for n in [-1, max + 1] {
                assert_eq!(registration(&format!("<key>StartCalendarInterval</key><dict><key>{key}</key><integer>{n}</integer></dict>")).unwrap_err(),DiagnosticCode::InvalidRegistration);
            }
        }
    }

    #[test]
    fn daily_weekly_and_wildcard_calendars_remain_distinct() {
        let r = registration("<key>StartCalendarInterval</key><array><dict><key>Hour</key><integer>18</integer><key>Minute</key><integer>30</integer></dict><dict><key>Weekday</key><integer>6</integer><key>Hour</key><integer>9</integer></dict><dict/></array>").unwrap();
        assert_eq!(r.kind, WorkKind::Schedule);
        assert_eq!(r.triggers.len(), 3);
        assert!(matches!(
            &r.triggers[0],
            Trigger::Calendar {
                components: Calendar {
                    hour: Some(18),
                    minute: Some(30),
                    weekday: None,
                    ..
                }
            }
        ));
        assert!(matches!(
            &r.triggers[1],
            Trigger::Calendar {
                components: Calendar {
                    weekday: Some(6),
                    ..
                }
            }
        ));
        assert!(matches!(
            &r.triggers[2],
            Trigger::Calendar {
                components: Calendar {
                    minute: None,
                    hour: None,
                    ..
                }
            }
        ));
    }

    #[test]
    fn conditional_keepalive_and_unknown_provider_features_need_review() {
        let r = registration("<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict><key>WatchPaths</key><array><string>/tmp/work</string></array>").unwrap();
        assert_eq!(r.kind, WorkKind::Service);
        assert!(r.requirements.contains(&Requirement::ConditionalRestart));
        assert!(
            r.requirements
                .contains(&Requirement::AdditionalLaunchdBehavior)
        );
    }

    #[test]
    fn validates_supported_fields_and_rejects_bad_data_without_source_echo() {
        for extra in [
            "<key>StartInterval</key><integer>0</integer>",
            "<key>StartInterval</key><integer>-5</integer>",
            "<key>RunAtLoad</key><string>true</string>",
            "<key>KeepAlive</key><string>true</string>",
            "<key>StartCalendarInterval</key><dict><key>Hour</key><integer>24</integer></dict>",
            "<key>StartCalendarInterval</key><dict><key>Month</key><integer>0</integer></dict>",
            "<key>StartCalendarInterval</key><dict><key>Minute</key><string>secret-value</string></dict>",
            "<key>ProgramArguments</key><array/>",
            "<key>EnvironmentVariables</key><dict><key>A</key><integer>7</integer></dict>",
        ] {
            assert_eq!(
                registration(extra).unwrap_err(),
                DiagnosticCode::InvalidRegistration
            );
        }
    }

    #[test]
    fn only_environment_count_leaves_adapter() {
        let r = registration("<key>ProgramArguments</key><array><string>/bin/sh</string><string>-c</string><string>secret-argument</string></array><key>EnvironmentVariables</key><dict><key>PRIVATE</key><string>secret-value</string></dict>").unwrap();
        let serialized = serde_json::to_string(&r).unwrap();
        assert_eq!(r.environment_variable_count, 1);
        for secret in ["secret-argument", "secret-value", "PRIVATE", "/bin/sh"] {
            assert!(!serialized.contains(secret));
        }
    }
}
