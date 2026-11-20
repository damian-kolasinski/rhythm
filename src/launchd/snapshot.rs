//! Extract only scheduler facts from a read-only launchctl dump.
use super::{definition, print};
use crate::jobs::{Observation, Trigger};
use std::{collections::BTreeMap, path::PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(super) enum Scope {
    Agent,
    Daemon,
}

pub(super) type Identity = (Scope, String);

pub(super) struct LiveJob {
    pub path: Option<PathBuf>,
    pub observation: Observation,
    pub timers: Vec<Trigger>,
    graphical: bool,
}

impl LiveJob {
    pub fn add_timers(&mut self, other: Self) {
        for timer in other.timers {
            if !self.timers.contains(&timer) {
                self.timers.push(timer);
            }
        }
    }
}

#[derive(Default)]
pub(super) struct LoadedJobs {
    pub jobs: BTreeMap<Identity, Vec<LiveJob>>,
    agents: bool,
    daemons: bool,
}

impl LoadedJobs {
    pub fn covers(&self, scope: Scope) -> bool {
        match scope {
            Scope::Agent => self.agents,
            Scope::Daemon => self.daemons,
        }
    }

    pub fn parse(text: &str, uid: u32) -> Self {
        let user = format!("user/{uid}");
        let gui = format!("gui/{uid}");
        let mut loaded = Self::default();
        for block in text.split("\n}\n") {
            let block = block.trim_start_matches('\n');
            let (header, body) = block.split_once('\n').unwrap_or((block, ""));
            let Some(header) = header.strip_suffix(" = {") else {
                continue;
            };
            if header == "system" {
                loaded.daemons = true;
                continue;
            }
            if header == gui || header == user {
                loaded.agents = true;
                continue;
            }
            let Some((domain, label)) = header.rsplit_once('/') else {
                continue;
            };
            let (scope, graphical) = match domain {
                "system" => (Scope::Daemon, false),
                domain if domain == gui => (Scope::Agent, true),
                domain if domain == user => (Scope::Agent, false),
                _ => continue,
            };
            // Read-only labels do not have to meet Rhythm's authoring rules.
            if label.is_empty() || label.len() > 1024 || label.chars().any(char::is_control) {
                continue;
            }
            loaded
                .jobs
                .entry((scope, label.to_owned()))
                .or_default()
                .push(LiveJob {
                    path: print::path(body).filter(|path| path.is_absolute()),
                    observation: print::parse(body),
                    timers: timers(body),
                    graphical,
                });
        }
        for registrations in loaded.jobs.values_mut() {
            // Prefer GUI observations only once matching paths prove a shared job.
            registrations.sort_by_key(|job| !job.graphical);
        }
        loaded
    }
}

fn timers(body: &str) -> Vec<Trigger> {
    let mut result = Vec::new();
    if let Some(seconds) = body.lines().find_map(|line| {
        line.strip_prefix("\trun interval = ")?
            .strip_suffix(" seconds")?
            .parse::<u64>()
            .ok()
            .filter(|s| *s > 0)
    }) {
        result.push(Trigger::Interval { seconds });
    }
    let events = body
        .split_once("\tevent triggers = {\n")
        .and_then(|(_, rest)| rest.split_once("\n\t}"))
        .map_or("", |(events, _)| events);
    for event in events.split("\n\t\t}") {
        let Some((meta, descriptor)) = event.split_once("\n\t\t\tdescriptor = {\n") else {
            continue;
        };
        if !meta
            .lines()
            .any(|line| line == "\t\t\tstream = com.apple.launchd.calendarinterval")
        {
            continue;
        }
        let fields: Option<plist::Dictionary> = descriptor
            .lines()
            .map_while(|line| line.strip_prefix("\t\t\t\t"))
            .map(|line| {
                let (name, value) = line.split_once(" => ")?;
                Some((
                    name.strip_prefix('"')?.strip_suffix('"')?.to_owned(),
                    plist::Value::from(value.parse::<u64>().ok()?),
                ))
            })
            .collect();
        if let Some(calendar) = fields.as_ref().and_then(definition::calendar) {
            result.push(Trigger::Calendar(calendar));
        }
    }
    result
}
