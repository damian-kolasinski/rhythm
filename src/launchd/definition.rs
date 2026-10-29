//! Read a launchd property list into the job's label and activation.
use crate::{
    jobs::{Activation, Calendar, RunStore, Trigger},
    wrapper_command::Wrapped,
};
use plist::{Dictionary, Value};

pub struct Definition {
    pub label: String,
    pub activation: Activation,
    /// Runs through `rhythm run` under its own label.
    pub recorded: Option<RunStore>,
}

pub enum Parsed {
    Job(Definition),
    /// An empty dictionary, which some installers leave to disable an agent.
    Empty,
    /// Anything else launchd itself would refuse to load.
    Invalid,
}

pub fn parse(bytes: &[u8]) -> Parsed {
    match Value::from_reader(std::io::Cursor::new(bytes)) {
        Ok(Value::Dictionary(plist)) if plist.is_empty() => Parsed::Empty,
        Ok(Value::Dictionary(plist)) => job(&plist).map_or(Parsed::Invalid, Parsed::Job),
        _ => Parsed::Invalid,
    }
}

fn job(plist: &Dictionary) -> Option<Definition> {
    let label = plist.get("Label")?.as_string()?.to_owned();
    let program = plist.get("ProgramArguments").and_then(Value::as_array);
    if label.is_empty()
        || !(plist.contains_key("Program") || program.is_some_and(|a| !a.is_empty()))
    {
        return None;
    }
    let activation = match plist.get("KeepAlive") {
        Some(Value::Boolean(true)) => Activation::Service { always: true },
        Some(Value::Dictionary(conditions)) if !conditions.is_empty() => {
            Activation::Service { always: false }
        }
        _ => Activation::Triggered {
            triggers: triggers(plist)?,
        },
    };
    let arguments: Option<Vec<String>> =
        program.and_then(|a| a.iter().map(|v| v.as_string().map(str::to_owned)).collect());
    // With `Program`, launchd runs that file and ProgramArguments is only argv.
    let recorded = arguments
        .filter(|_| !plist.contains_key("Program"))
        .as_deref()
        .and_then(Wrapped::parse)
        .filter(|w| w.label.as_str() == label)
        .map(|w| w.data_dir.map_or(RunStore::Default, RunStore::Directory));
    Some(Definition {
        label,
        activation,
        recorded,
    })
}

fn triggers(plist: &Dictionary) -> Option<Vec<Trigger>> {
    let mut triggers = vec![];
    if flag(plist, "RunAtLoad") {
        triggers.push(Trigger::AtLoad);
    }
    if let Some(seconds) = plist.get("StartInterval") {
        let seconds = seconds.as_unsigned_integer().filter(|s| *s > 0)?;
        triggers.push(Trigger::Interval { seconds });
    }
    match plist.get("StartCalendarInterval") {
        None => {}
        Some(Value::Array(entries)) => {
            for entry in entries {
                triggers.push(Trigger::Calendar(calendar(entry.as_dictionary()?)?));
            }
        }
        Some(entry) => triggers.push(Trigger::Calendar(calendar(entry.as_dictionary()?)?)),
    }
    for (key, trigger) in [
        ("WatchPaths", Trigger::PathChange),
        ("QueueDirectories", Trigger::QueueDirectory),
        ("Sockets", Trigger::Socket),
    ] {
        if plist.contains_key(key) {
            triggers.push(trigger);
        }
    }
    if flag(plist, "StartOnMount") {
        triggers.push(Trigger::Mount);
    }
    Some(triggers)
}

fn flag(plist: &Dictionary, key: &str) -> bool {
    plist.get(key).and_then(Value::as_boolean) == Some(true)
}

fn calendar(entry: &Dictionary) -> Option<Calendar> {
    let field = |key: &str, max: u64| -> Option<Option<u8>> {
        match entry.get(key) {
            None => Some(None),
            Some(value) => Some(Some(
                value
                    .as_unsigned_integer()
                    .filter(|v| *v <= max)
                    .and_then(|v| u8::try_from(v).ok())?,
            )),
        }
    };
    Some(Calendar {
        minute: field("Minute", 59)?,
        hour: field("Hour", 23)?,
        day: field("Day", 31)?,
        weekday: field("Weekday", 7)?,
        month: field("Month", 12)?,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn xml(label: &str, body: &str) -> Vec<u8> {
        format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string><key>Program</key><string>/bin/true</string>{body}</dict></plist>"#
        )
        .into_bytes()
    }

    fn activation(body: &str) -> Option<Activation> {
        match parse(&xml("com.example.job", body)) {
            Parsed::Job(d) => Some(d.activation),
            _ => None,
        }
    }

    #[test]
    fn reads_keep_alive_services() {
        assert_eq!(
            activation("<key>KeepAlive</key><true/><key>RunAtLoad</key><true/>"),
            Some(Activation::Service { always: true })
        );
        assert_eq!(
            activation("<key>KeepAlive</key><dict><key>SuccessfulExit</key><false/></dict>"),
            Some(Activation::Service { always: false })
        );
    }

    #[test]
    fn reads_every_trigger_kind() {
        let parsed = activation(
            "<key>RunAtLoad</key><true/>\
             <key>StartInterval</key><integer>300</integer>\
             <key>StartCalendarInterval</key><array>\
               <dict><key>Weekday</key><integer>1</integer><key>Hour</key><integer>9</integer><key>Minute</key><integer>15</integer></dict>\
               <dict><key>Minute</key><integer>0</integer></dict>\
             </array>\
             <key>WatchPaths</key><array><string>/tmp/x</string></array>\
             <key>QueueDirectories</key><array><string>/tmp/q</string></array>\
             <key>StartOnMount</key><true/>\
             <key>KeepAlive</key><false/>",
        );
        assert_eq!(
            parsed,
            Some(Activation::Triggered {
                triggers: vec![
                    Trigger::AtLoad,
                    Trigger::Interval { seconds: 300 },
                    Trigger::Calendar(Calendar {
                        minute: Some(15),
                        hour: Some(9),
                        weekday: Some(1),
                        ..Calendar::default()
                    }),
                    Trigger::Calendar(Calendar {
                        minute: Some(0),
                        ..Calendar::default()
                    }),
                    Trigger::PathChange,
                    Trigger::QueueDirectory,
                    Trigger::Mount,
                ]
            })
        );
    }

    #[test]
    fn a_single_calendar_dictionary_is_one_trigger() {
        assert_eq!(
            activation("<key>StartCalendarInterval</key><dict><key>Hour</key><integer>8</integer><key>Minute</key><integer>7</integer></dict>")
                .unwrap()
                .to_string(),
            "daily 08:07"
        );
    }

    #[test]
    fn refuses_definitions_launchd_would_refuse() {
        for body in [
            "<key>StartInterval</key><integer>0</integer>",
            "<key>StartInterval</key><string>5</string>",
            "<key>StartCalendarInterval</key><dict><key>Hour</key><integer>24</integer></dict>",
            "<key>StartCalendarInterval</key><array><string>daily</string></array>",
        ] {
            assert_eq!(activation(body), None, "{body}");
        }
        assert!(matches!(parse(b"not a plist"), Parsed::Invalid));
        assert!(matches!(parse(&xml("", "")), Parsed::Invalid));
        let no_program = br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>com.example.job</string><key>ProgramArguments</key><array/></dict></plist>"#;
        assert!(matches!(parse(no_program), Parsed::Invalid));
    }

    #[test]
    fn recognizes_jobs_recorded_under_their_own_label() {
        let wrapped = |label: &str| {
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>com.example.job</string><key>ProgramArguments</key><array><string>/opt/bin/rhythm</string><string>run</string><string>{label}</string><string>--</string><string>/bin/true</string></array></dict></plist>"#
            )
        };
        let recorded =
            |bytes: &[u8]| matches!(parse(bytes), Parsed::Job(d) if d.recorded.is_some());
        assert!(recorded(wrapped("com.example.job").as_bytes()));
        assert!(!recorded(wrapped("com.example.other").as_bytes()));
        assert!(!recorded(&xml("com.example.job", "")));
        // With Program, launchd runs that file; ProgramArguments is only argv.
        let program_key = wrapped("com.example.job").replace(
            "<key>ProgramArguments</key>",
            "<key>Program</key><string>/bin/true</string><key>ProgramArguments</key>",
        );
        assert!(!recorded(program_key.as_bytes()));
    }

    #[test]
    fn an_empty_dictionary_is_not_a_job() {
        let empty =
            br#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict/></plist>"#;
        assert!(matches!(parse(empty), Parsed::Empty));
    }
}
