//! Write job definitions as launchd property lists.
use super::launchctl::Domain;
use crate::{
    jobs::{JobSpec, Schedule},
    operation_error::{Error, Result},
};
use plist::{Dictionary, Value};

/// A new job's definition for loading into `domain`.
pub fn job(spec: &JobSpec, domain: Domain) -> Result<Vec<u8>> {
    let mut plist = Dictionary::new();
    plist.insert("Label".into(), spec.label.to_string().into());
    plist.insert("ProgramArguments".into(), strings(&spec.arguments));
    plist.insert(
        "WorkingDirectory".into(),
        path(&spec.working_directory)?.into(),
    );
    plist.insert(
        "EnvironmentVariables".into(),
        Value::Dictionary(
            spec.environment
                .iter()
                .map(|(k, v)| (k.clone(), Value::from(v.as_str())))
                .collect(),
        ),
    );
    let diagnostics = path(&spec.diagnostics)?;
    plist.insert("StandardOutPath".into(), diagnostics.into());
    plist.insert("StandardErrorPath".into(), diagnostics.into());
    match &spec.schedule {
        Schedule::KeepAlive => {
            plist.insert("KeepAlive".into(), true.into());
        }
        Schedule::Triggered {
            at_load,
            every_seconds,
            calendar,
        } => {
            if *at_load {
                plist.insert("RunAtLoad".into(), true.into());
            }
            if let Some(seconds) = every_seconds {
                plist.insert("StartInterval".into(), Value::from(*seconds));
            }
            if !calendar.is_empty() {
                let entries = calendar
                    .iter()
                    .map(|entry| {
                        let mut fields = Dictionary::new();
                        for (key, value) in [
                            ("Minute", entry.minute),
                            ("Hour", entry.hour),
                            ("Day", entry.day),
                            ("Weekday", entry.weekday),
                            ("Month", entry.month),
                        ] {
                            if let Some(value) = value {
                                fields.insert(key.into(), Value::from(u64::from(value)));
                            }
                        }
                        Value::Dictionary(fields)
                    })
                    .collect();
                plist.insert("StartCalendarInterval".into(), Value::Array(entries));
            }
        }
    }
    if domain == Domain::User {
        plist.insert(
            "LimitLoadToSessionType".into(),
            domain.session_type().into(),
        );
    }
    xml(plist)
}

/// The same definition starting `arguments` instead.
pub fn with_program(mut plist: Dictionary, arguments: &[String]) -> Result<Vec<u8>> {
    plist.remove("Program");
    plist.insert("ProgramArguments".into(), strings(arguments));
    xml(plist)
}

pub fn xml(plist: Dictionary) -> Result<Vec<u8>> {
    let mut bytes = vec![];
    plist::to_writer_xml(&mut bytes, &Value::Dictionary(plist))
        .map_err(|_| Error::InvalidRequest)?;
    Ok(bytes)
}

fn strings(values: &[String]) -> Value {
    Value::Array(values.iter().map(|v| Value::from(v.as_str())).collect())
}

fn path(path: &std::path::Path) -> Result<&str> {
    path.to_str()
        .filter(|_| path.is_absolute())
        .ok_or(Error::InvalidRequest)
}
