//! The desktop inventory of periodic jobs and jobs already recorded by Rhythm.
use crate::{
    authoring::OWNED_PREFIX,
    cron::Cron,
    jobs::{Health, Job, Trigger},
    launchd::{Launchd, Periodic},
    operation_error::Result,
    status::{self, JobReport, ReportReader},
};
use std::collections::BTreeSet;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ownership {
    Created,
    Adopted,
    External,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Source {
    UserLaunchAgents,
    SystemLaunchdDefinitions,
    LaunchdStatus,
    UserCrontab,
    SystemCrontabs,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum LaunchdSource {
    UserAgent,
    Agent,
    Daemon,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CronTable {
    User,
    System,
    Other(usize),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CronEntry {
    pub table: CronTable,
    pub line: usize,
    pub schedule: String,
}

// An inventory holds a few dozen routines, built once per refresh.
#[allow(clippy::large_enum_variant)]
#[derive(Debug)]
pub enum Routine {
    Launchd {
        source: LaunchdSource,
        /// Distinguishes separate definitions with the same label in one scope.
        instance: usize,
        report: JobReport,
        /// Timers alongside KeepAlive, or timers found only in live state.
        additional_timers: Vec<Trigger>,
    },
    Cron(CronEntry),
}

impl Routine {
    pub fn ownership(&self) -> Ownership {
        match self {
            Self::Launchd {
                source: LaunchdSource::UserAgent,
                report,
                ..
            } if report.recorded => {
                if report.label.starts_with(OWNED_PREFIX) {
                    Ownership::Created
                } else {
                    Ownership::Adopted
                }
            }
            _ => Ownership::External,
        }
    }

    pub fn health(&self) -> Health {
        match self {
            Self::Launchd { report, .. } => report.health,
            Self::Cron(_) => Health::Unknown,
        }
    }
}

pub struct Snapshot {
    pub routines: Vec<Routine>,
    /// Sources where some definitions or observations could not be read.
    pub incomplete: BTreeSet<Source>,
}

pub(crate) struct LaunchdEntry {
    pub source: LaunchdSource,
    pub instance: usize,
    pub job: Job,
    pub additional_timers: Vec<Trigger>,
}

#[derive(Default)]
pub(crate) struct Scan {
    pub launchd: Vec<LaunchdEntry>,
    pub cron: Vec<CronEntry>,
    pub incomplete: BTreeSet<Source>,
}

pub(crate) trait Provider {
    fn scan(&self) -> Scan;
}

pub fn current() -> Result<Snapshot> {
    let launchd = Periodic::current(Launchd::current_user()?);
    let cron = Cron::current();
    let (launchd, cron) = std::thread::scope(|scope| {
        let cron = scope.spawn(|| cron.scan());
        (launchd.scan(), cron.join().expect("cron inventory worker"))
    });
    snapshot([launchd, cron])
}

fn snapshot(scans: impl IntoIterator<Item = Scan>) -> Result<Snapshot> {
    let ledgers = status::local_ledgers(None);
    let mut reader = ReportReader::new(&ledgers);
    let mut routines = Vec::new();
    let mut incomplete = BTreeSet::new();
    for scan in scans {
        incomplete.extend(scan.incomplete);
        for entry in scan.launchd {
            routines.push(Routine::Launchd {
                source: entry.source,
                instance: entry.instance,
                report: reader.read(entry.job)?,
                additional_timers: entry.additional_timers,
            });
        }
        routines.extend(scan.cron.into_iter().map(Routine::Cron));
    }
    // The adapters provide stable order within a health state.
    routines.sort_by_key(Routine::health);
    Ok(Snapshot {
        routines,
        incomplete,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jobs::{Exit, Observation};
    use std::{
        fs,
        os::unix::fs::PermissionsExt,
        path::{Path, PathBuf},
    };

    fn launchctl(dir: &Path, fixture: &str) -> PathBuf {
        let launchctl = dir.join("launchctl");
        fs::write(
            &launchctl,
            "#!/bin/sh\n[ \"$1\" = dumpstate ] && /bin/cat \"$0.fixture\"\n",
        )
        .unwrap();
        fs::set_permissions(&launchctl, fs::Permissions::from_mode(0o700)).unwrap();
        fs::write(launchctl.with_extension("fixture"), fixture).unwrap();
        launchctl
    }

    fn definition(dir: &Path, label: &str, recorder: bool, triggers: &str) {
        let program = if recorder {
            format!(
                "<key>ProgramArguments</key><array><string>/opt/bin/rhythm</string><string>run</string><string>{label}</string><string>--</string><string>/usr/bin/true</string></array>"
            )
        } else {
            "<key>Program</key><string>/usr/bin/true</string>".into()
        };
        fs::write(dir.join(format!("{label}.plist")), format!(
            r#"<?xml version="1.0" encoding="UTF-8"?><plist version="1.0"><dict><key>Label</key><string>{label}</string>{program}{triggers}</dict></plist>"#,
        )).unwrap();
    }

    #[test]
    fn combines_installed_and_live_schedules_with_recorded_on_demand_jobs() {
        let scratch = tempfile::tempdir().unwrap();
        let agents = scratch.path().join("UserAgents");
        let machine = scratch.path().join("MachineAgents");
        let daemons = scratch.path().join("Daemons");
        for path in [&agents, &machine, &daemons] {
            fs::create_dir(path).unwrap();
        }
        let interval = "<key>StartInterval</key><integer>300</integer>";
        definition(&agents, "dev.rhythm.manual", true, "");
        definition(&agents, "com.example.adopted", true, interval);
        definition(&agents, "dev.rhythm.external", false, interval);
        definition(
            &agents,
            "com.example.at-load",
            false,
            "<key>RunAtLoad</key><true/>",
        );
        definition(&machine, "com.example.machine", false, interval);
        definition(
            &daemons,
            "com.example.service",
            false,
            &format!("<key>KeepAlive</key><true/>{interval}"),
        );
        fs::write(daemons.join("unreadable.plist"), "unreadable").unwrap();
        let fixture = concat!(
            "system = {\n\ttype = system\n}\n",
            "gui/501 = {\n\ttype = login\n}\n",
            "system/com.example.machine = {\n\truns = 0\n}\n",
            "gui/501/com.example.adopted = {\n\truns = 3\n\tlast exit code = 1\n}\n",
            "gui/501/com.example.app-agent = {\n\truns = 1\n\tlast exit code = 0\n\tevent triggers = {\n",
            "\t\tapp.1 => {\n\t\t\tstream = com.apple.launchd.calendarinterval\n\t\t\tdescriptor = {\n",
            "\t\t\t\t\"Hour\" => 14\n\t\t\t\t\"Minute\" => 30\n\t\t\t}\n\t\t}\n\t}\n}\n",
            "user/502/com.example.other-user = {\n\trun interval = 60 seconds\n}\n",
            "system/com.example.live-only = {\n\truns = 0\n\trun interval = 60 seconds\n}\n",
        );
        let provider = Periodic::at_boundary(
            Launchd::at_boundary(agents.clone(), launchctl(scratch.path(), fixture), 501),
            vec![
                (LaunchdSource::UserAgent, agents),
                (LaunchdSource::Agent, machine),
                (LaunchdSource::Daemon, daemons),
            ],
        );
        let snapshot = snapshot([provider.scan()]).unwrap();
        assert_eq!(
            snapshot.incomplete,
            [Source::SystemLaunchdDefinitions].into()
        );
        let jobs: Vec<_> = snapshot
            .routines
            .iter()
            .map(|routine| {
                let Routine::Launchd { report, .. } = routine else {
                    panic!("expected launchd routine")
                };
                (report.label.as_str(), routine.ownership(), report.health)
            })
            .collect();
        assert_eq!(
            jobs,
            [
                ("com.example.adopted", Ownership::Adopted, Health::Failing),
                ("com.example.app-agent", Ownership::External, Health::Ok),
                ("com.example.live-only", Ownership::External, Health::Idle),
                ("com.example.machine", Ownership::External, Health::Unloaded),
                ("com.example.service", Ownership::External, Health::Unloaded),
                ("dev.rhythm.external", Ownership::External, Health::Unloaded),
                ("dev.rhythm.manual", Ownership::Created, Health::Unloaded),
            ]
        );
        assert!(matches!(
            &snapshot.routines[1], Routine::Launchd { report, .. }
                if report.activation.as_ref().unwrap().to_string() == "daily 14:30"
        ));
        assert!(matches!(
            &snapshot.routines[4], Routine::Launchd { additional_timers, .. }
                if additional_timers == &[Trigger::Interval { seconds: 300 }]
        ));
        assert!(matches!(
            &snapshot.routines[0], Routine::Launchd { report, .. }
                if matches!(report.observation, Observation::Loaded { last_exit: Some(Exit::Code { code: 1 }), .. })
        ));
    }

    #[test]
    fn a_shared_label_keeps_distinct_paths_timers_health_and_ownership() {
        let scratch = tempfile::tempdir().unwrap();
        let agents = scratch.path().join("UserAgents");
        fs::create_dir(&agents).unwrap();
        let label = "dev.rhythm.shared";
        definition(
            &agents,
            label,
            true,
            "<key>StartInterval</key><integer>60</integer>",
        );
        let path = agents.join(format!("{label}.plist"));
        let fixture = format!(
            "system = {{\n}}\ngui/501 = {{\n}}\nuser/501 = {{\n}}\n\
            gui/501/{label} = {{\n\tpath = /Applications/Example.app/agent.plist\n\tlast exit code = 2\n\trun interval = 600 seconds\n}}\n\
            user/501/{label} = {{\n\tpath = {}\n\tlast exit code = 0\n\trun interval = 60 seconds\n}}\n\
            gui/501/com.example.café report = {{\n\trun interval = 300 seconds\n}}\n",
            path.display()
        );
        let provider = Periodic::at_boundary(
            Launchd::at_boundary(agents.clone(), launchctl(scratch.path(), &fixture), 501),
            vec![(LaunchdSource::UserAgent, agents)],
        );
        let actual = snapshot([provider.scan()]).unwrap();
        assert!(actual.incomplete.is_empty());
        let rows: Vec<_> = actual
            .routines
            .iter()
            .map(|routine| {
                let Routine::Launchd {
                    instance, report, ..
                } = routine
                else {
                    panic!("expected launchd routine")
                };
                (
                    report.label.as_str(),
                    *instance,
                    routine.ownership(),
                    report.health,
                    report.activation.as_ref().unwrap().to_string(),
                )
            })
            .collect();
        assert_eq!(
            rows,
            [
                (
                    label,
                    1,
                    Ownership::External,
                    Health::Failing,
                    "every 10m".into()
                ),
                (label, 0, Ownership::Created, Health::Ok, "every 1m".into()),
                (
                    "com.example.café report",
                    0,
                    Ownership::External,
                    Health::Idle,
                    "every 5m".into()
                ),
            ]
        );
    }

    #[test]
    fn retains_schedules_without_claiming_health_when_launchd_cannot_be_queried() {
        let scratch = tempfile::tempdir().unwrap();
        let agents = scratch.path().join("agents");
        fs::create_dir(&agents).unwrap();
        definition(
            &agents,
            "com.example.external",
            false,
            "<key>StartInterval</key><integer>10</integer>",
        );
        let provider = Periodic::at_boundary(
            Launchd::at_boundary(agents.clone(), scratch.path().join("missing"), 501),
            vec![(LaunchdSource::UserAgent, agents)],
        );
        let snapshot = snapshot([provider.scan()]).unwrap();
        assert_eq!(snapshot.incomplete, [Source::LaunchdStatus].into());
        assert_eq!(snapshot.routines.len(), 1);
        assert_eq!(snapshot.routines[0].health(), Health::Unknown);
    }
}
