//! Merge installed definitions with live timers, including app-bundled agents.
use super::{
    Launchd, definition, definitions, read,
    snapshot::{Identity, LiveJob, Scope},
};
use crate::{
    jobs::{Activation, Job, Observation},
    routines::{LaunchdEntry, LaunchdSource, Provider, Scan, Source},
};
use std::{
    collections::BTreeMap,
    path::{Path, PathBuf},
};

pub(crate) struct Periodic {
    launchd: Launchd,
    roots: Vec<(LaunchdSource, PathBuf)>,
}

impl Periodic {
    pub fn current(launchd: Launchd) -> Self {
        let roots = vec![
            (LaunchdSource::UserAgent, launchd.agents.clone()),
            (LaunchdSource::Agent, "/Library/LaunchAgents".into()),
            (LaunchdSource::Agent, "/System/Library/LaunchAgents".into()),
            (LaunchdSource::Daemon, "/Library/LaunchDaemons".into()),
            (
                LaunchdSource::Daemon,
                "/System/Library/LaunchDaemons".into(),
            ),
        ];
        Self { launchd, roots }
    }

    #[cfg(test)]
    pub(crate) fn at_boundary(launchd: Launchd, roots: Vec<(LaunchdSource, PathBuf)>) -> Self {
        Self { launchd, roots }
    }
}

impl Provider for Periodic {
    fn scan(&self) -> Scan {
        let mut scan = Scan::default();
        let mut live = self.launchd.launchctl.snapshot().unwrap_or_default();
        if !live.covers(Scope::Agent) || !live.covers(Scope::Daemon) {
            scan.incomplete.insert(Source::LaunchdStatus);
        }
        let mut groups: BTreeMap<Identity, Registrations> = BTreeMap::new();
        for (source, directory) in &self.roots {
            let unknown = match source {
                LaunchdSource::UserAgent => Source::UserLaunchAgents,
                _ => Source::SystemLaunchdDefinitions,
            };
            let Ok(listing) = definitions(directory) else {
                scan.incomplete.insert(unknown);
                continue;
            };
            if listing.incomplete {
                scan.incomplete.insert(unknown);
            }
            for path in listing.paths {
                let definition = match read(&path).map_or(definition::Parsed::Invalid, |bytes| {
                    definition::parse(&bytes)
                }) {
                    definition::Parsed::Job(definition) => definition,
                    definition::Parsed::Empty => continue,
                    definition::Parsed::Invalid => {
                        scan.incomplete.insert(unknown);
                        continue;
                    }
                };
                let identity = (scope(*source), definition.label.clone());
                let installed = &mut groups.entry(identity).or_default().installed;
                if !installed
                    .iter()
                    .any(|entry| same_definition(&entry.path, &path))
                {
                    installed.push(Installed {
                        source: *source,
                        definition,
                        path,
                    });
                }
            }
        }
        for (identity, registrations) in std::mem::take(&mut live.jobs) {
            groups.entry(identity).or_default().loaded = registrations;
        }
        for ((scope, label), registrations) in groups {
            let absent = if live.covers(scope) {
                Observation::Unloaded
            } else {
                Observation::Unavailable
            };
            registrations.append(scope, &label, absent, &mut scan.launchd);
        }
        scan.launchd.sort_by(|a, b| a.job.label.cmp(&b.job.label));
        scan
    }
}

struct Installed {
    source: LaunchdSource,
    path: PathBuf,
    definition: definition::Definition,
}

#[derive(Default)]
struct Registrations {
    installed: Vec<Installed>,
    loaded: Vec<LiveJob>,
}

impl Registrations {
    fn append(
        self,
        scope: Scope,
        label: &str,
        absent: Observation,
        output: &mut Vec<LaunchdEntry>,
    ) {
        let Self {
            installed,
            mut loaded,
        } = self;
        let unambiguous = installed.len() == 1 && loaded.len() == 1;
        let mut instance = 0;
        for entry in installed {
            let observed = take_matching(&mut loaded, &entry.path, unambiguous);
            output.extend(entry.routine(instance, observed, &absent));
            instance += 1;
        }
        while !loaded.is_empty() {
            let mut job = loaded.remove(0);
            if let Some(path) = &job.path
                && let Some(shared) = take_matching(&mut loaded, path, false)
            {
                job.add_timers(shared);
            }
            if job.timers.is_empty() {
                continue;
            }
            let source = match scope {
                Scope::Agent => LaunchdSource::Agent,
                Scope::Daemon => LaunchdSource::Daemon,
            };
            output.push(LaunchdEntry {
                source,
                instance,
                job: Job {
                    label: label.to_owned(),
                    activation: Some(Activation::Triggered {
                        triggers: job.timers,
                    }),
                    recorded: None,
                    observation: job.observation,
                },
                additional_timers: Vec::new(),
            });
            instance += 1;
        }
    }
}

impl Installed {
    fn routine(
        self,
        instance: usize,
        observed: Option<LiveJob>,
        absent: &Observation,
    ) -> Option<LaunchdEntry> {
        let Self {
            source, definition, ..
        } = self;
        let mut additional_timers = definition.service_timers;
        if !definition.activation.is_periodic()
            && additional_timers.is_empty()
            && let Some(loaded) = &observed
        {
            additional_timers.clone_from(&loaded.timers);
        }
        let recorded = (source == LaunchdSource::UserAgent)
            .then_some(definition.recorded)
            .flatten();
        if !definition.activation.is_periodic()
            && additional_timers.is_empty()
            && recorded.is_none()
        {
            return None;
        }
        Some(LaunchdEntry {
            source,
            instance,
            job: Job {
                label: definition.label,
                activation: Some(definition.activation),
                recorded,
                observation: observed.map_or_else(|| absent.clone(), |loaded| loaded.observation),
            },
            additional_timers,
        })
    }
}

fn take_matching(jobs: &mut Vec<LiveJob>, path: &Path, unambiguous: bool) -> Option<LiveJob> {
    let mut matches = jobs.extract_if(.., |job| match &job.path {
        Some(loaded) => same_definition(loaded, path),
        None => unambiguous,
    });
    let mut job = matches.next()?;
    for shared in matches {
        job.add_timers(shared);
    }
    Some(job)
}

fn same_definition(left: &Path, right: &Path) -> bool {
    left == right
        || left
            .canonicalize()
            .ok()
            .is_some_and(|resolved| right.canonicalize().ok().as_ref() == Some(&resolved))
}

fn scope(source: LaunchdSource) -> Scope {
    match source {
        LaunchdSource::Daemon => Scope::Daemon,
        _ => Scope::Agent,
    }
}
