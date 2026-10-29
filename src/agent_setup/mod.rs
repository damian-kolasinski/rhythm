//! Setting coding agents up to use Rhythm, as mergegate sets them up for its
//! hub: a skill stub where each agent loads skills, pointing at the guide this
//! Rhythm embeds, and a session hook in those that run one. Setup changes only
//! what a Rhythm wrote, and reports each item with what happened to it; one
//! that fails leaves the rest to go ahead.
mod agent;
mod agent_files;
mod hook;
mod stub;

pub use agent::Agent;
pub use hook::command as hook_command;

use crate::skill_guide::NAME;
use agent_files::Replaced;
use serde::Serialize;
use std::{
    fs, io,
    path::{Path, PathBuf},
};

/// The user whose agents are set up.
#[derive(Debug, Clone)]
pub struct Scope {
    home: PathBuf,
}
impl Scope {
    pub fn user(home: PathBuf) -> Self {
        Self { home }
    }
    /// The agents installed for this user, in [`Agent::ALL`]'s order.
    pub fn found(&self) -> Vec<Agent> {
        Agent::ALL
            .into_iter()
            .filter(|a| a.found(&self.home))
            .collect()
    }
    fn skill(&self, agent: Agent) -> PathBuf {
        self.home.join(agent.skills()).join(NAME)
    }
    fn hooks(&self, agent: Agent) -> Option<PathBuf> {
        agent.hooks().map(|file| self.home.join(file))
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Skill,
    Hook,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Change {
    Installed,
    Updated,
    Unchanged,
    Removed,
    /// There was nothing of ours to remove.
    Absent,
    /// A skill named rhythm that no Rhythm wrote, left as it was; `force` replaces it.
    Conflict,
    /// The same, which removing leaves alone.
    Kept,
    /// The agent can't hold this item, as an agent without command hooks a hook.
    Unsupported,
    /// Why it failed; nothing was changed.
    Failed(String),
}
impl Change {
    /// Whether the operator has something left to sort out.
    pub fn failed(&self) -> bool {
        matches!(self, Change::Conflict | Change::Failed(_))
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Step {
    /// The agents reading this item; agents sharing a folder share its step.
    pub agents: Vec<Agent>,
    pub kind: Kind,
    pub path: Option<PathBuf>,
    pub change: Change,
}
impl Step {
    /// What the operator does before a hook this step wrote runs.
    pub fn approval(&self) -> Option<&'static str> {
        match (self.kind, &self.change) {
            (Kind::Hook, Change::Installed | Change::Updated) => {
                self.agents.iter().find_map(|a| a.hook_approval())
            }
            _ => None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SkillState {
    /// As this Rhythm writes it.
    Current,
    /// A stub an older Rhythm wrote, or this version's edited by hand.
    Older,
    /// A stub a newer Rhythm wrote, which this one leaves alone.
    Newer,
    Missing,
    /// A skill named rhythm that no Rhythm wrote.
    Foreign,
    Unreadable(String),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HookState {
    Current,
    /// Ours, with another command, or more than once.
    Outdated,
    Missing,
    Unreadable(String),
    /// The agent runs no command hooks.
    Unsupported,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct AgentStatus {
    pub agent: Agent,
    pub found: bool,
    pub skill: PathBuf,
    pub skill_state: SkillState,
    pub hook: Option<PathBuf>,
    pub hook_state: HookState,
}
impl AgentStatus {
    pub fn ready(&self) -> bool {
        matches!(self.skill_state, SkillState::Current | SkillState::Newer)
            && matches!(self.hook_state, HookState::Current | HookState::Unsupported)
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Mode {
    Install {
        force: bool,
    },
    /// Brings up to date only what an older Rhythm installed.
    Refresh,
}

/// Installs the stub and the hook into `agents`, replacing what another
/// Rhythm installed. With `dry_run` it only says what it would do.
pub fn setup(scope: &Scope, agents: &[Agent], force: bool, dry_run: bool) -> Vec<Step> {
    apply(scope, agents, Mode::Install { force }, dry_run)
}

/// Updates the stubs and hooks an older Rhythm installed, and installs
/// nothing new, so an upgrade needs no setup. Only the steps that did something.
pub fn refresh(scope: &Scope) -> Vec<Step> {
    apply(scope, &Agent::ALL, Mode::Refresh, false)
        .into_iter()
        .filter(|step| step.change != Change::Unchanged)
        .collect()
}

fn apply(scope: &Scope, agents: &[Agent], mode: Mode, dry_run: bool) -> Vec<Step> {
    let mut steps = vec![];
    for (path, readers) in skill_folders(scope, agents) {
        let change = match (skill_state(&path), mode) {
            (SkillState::Current, _) => Change::Unchanged,
            (SkillState::Unreadable(why), _) => Change::Failed(why),
            (SkillState::Missing | SkillState::Foreign | SkillState::Newer, Mode::Refresh) => {
                continue;
            }
            (SkillState::Foreign, Mode::Install { force: false }) => Change::Conflict,
            (SkillState::Missing, _) => write_skill(&path, Change::Installed, dry_run),
            (SkillState::Older | SkillState::Newer | SkillState::Foreign, _) => {
                write_skill(&path, Change::Updated, dry_run)
            }
        };
        steps.push(Step {
            agents: readers,
            kind: Kind::Skill,
            path: Some(path),
            change,
        });
    }
    for &agent in agents {
        let Some(path) = scope.hooks(agent) else {
            if mode != Mode::Refresh {
                steps.push(Step {
                    agents: vec![agent],
                    kind: Kind::Hook,
                    path: None,
                    change: Change::Unsupported,
                });
            }
            continue;
        };
        let change = match agent_files::read_json(&path) {
            Err(why) => Change::Failed(why),
            Ok(read) => {
                let mut settings = read.value.clone().unwrap_or_else(|| serde_json::json!({}));
                match (hook::state(&settings, agent), mode) {
                    (hook::HookState::Current, _) => Change::Unchanged,
                    (hook::HookState::Missing, Mode::Refresh) => continue,
                    (state, _) => {
                        let done = match state {
                            hook::HookState::Missing => Change::Installed,
                            _ => Change::Updated,
                        };
                        match hook::install(&mut settings, agent) {
                            Err(why) => Change::Failed(why.into()),
                            Ok(()) if dry_run => done,
                            Ok(()) => {
                                replaced(agent_files::replace_json(&path, &read, &settings), done)
                            }
                        }
                    }
                }
            }
        };
        steps.push(Step {
            agents: vec![agent],
            kind: Kind::Hook,
            path: Some(path),
            change,
        });
    }
    steps
}

/// Removes the stub and hook from `agents`, leaving everything else as it was.
pub fn remove(scope: &Scope, agents: &[Agent], dry_run: bool) -> Vec<Step> {
    let mut steps = vec![];
    for (path, _) in skill_folders(scope, agents) {
        // Every agent reading the folder loses it, asked for or not.
        let readers = Agent::ALL
            .into_iter()
            .filter(|&a| scope.skill(a) == path)
            .collect();
        let change = match skill_state(&path) {
            SkillState::Current | SkillState::Older | SkillState::Newer if dry_run => {
                Change::Removed
            }
            SkillState::Current | SkillState::Older | SkillState::Newer => {
                match remove_skill(&path) {
                    Ok(()) => Change::Removed,
                    Err(e) => Change::Failed(e.to_string()),
                }
            }
            SkillState::Missing => Change::Absent,
            SkillState::Foreign => Change::Kept,
            SkillState::Unreadable(why) => Change::Failed(why),
        };
        steps.push(Step {
            agents: readers,
            kind: Kind::Skill,
            path: Some(path),
            change,
        });
    }
    for &agent in agents {
        let Some(path) = scope.hooks(agent) else {
            continue;
        };
        let change = match agent_files::read_json(&path) {
            Err(why) => Change::Failed(why),
            Ok(read) => match read.value.clone() {
                None => Change::Absent,
                Some(mut settings) => match hook::remove(&mut settings) {
                    Err(why) => Change::Failed(why.into()),
                    Ok(false) => Change::Absent,
                    Ok(true) if dry_run => Change::Removed,
                    Ok(true) => replaced(
                        agent_files::replace_json(&path, &read, &settings),
                        Change::Removed,
                    ),
                },
            },
        };
        steps.push(Step {
            agents: vec![agent],
            kind: Kind::Hook,
            path: Some(path),
            change,
        });
    }
    steps
}

/// Every agent Rhythm knows, found or not.
pub fn status(scope: &Scope) -> Vec<AgentStatus> {
    Agent::ALL
        .into_iter()
        .map(|agent| {
            let skill = scope.skill(agent);
            let hook = scope.hooks(agent);
            let hook_state = match &hook {
                None => HookState::Unsupported,
                Some(path) => match agent_files::read_json(path) {
                    Err(why) => HookState::Unreadable(why),
                    Ok(read) => match read.value {
                        None => HookState::Missing,
                        Some(settings) => match hook::state(&settings, agent) {
                            hook::HookState::Current => HookState::Current,
                            hook::HookState::Outdated => HookState::Outdated,
                            hook::HookState::Missing => HookState::Missing,
                        },
                    },
                },
            };
            AgentStatus {
                agent,
                found: agent.found(&scope.home),
                skill_state: skill_state(&skill),
                skill,
                hook,
                hook_state,
            }
        })
        .collect()
}

/// Each skill folder `agents` read, with the agents reading it.
fn skill_folders(scope: &Scope, agents: &[Agent]) -> Vec<(PathBuf, Vec<Agent>)> {
    let mut folders: Vec<(PathBuf, Vec<Agent>)> = vec![];
    for agent in Agent::ALL.into_iter().filter(|a| agents.contains(a)) {
        let path = scope.skill(agent);
        match folders.iter_mut().find(|(p, _)| *p == path) {
            Some((_, readers)) => readers.push(agent),
            None => folders.push((path, vec![agent])),
        }
    }
    folders
}

fn skill_state(path: &Path) -> SkillState {
    match fs::read_to_string(path.join("SKILL.md")) {
        Ok(text) if text == stub::text() => SkillState::Current,
        Ok(text) => match stub::version(&text) {
            Some(version) if version > stub::VERSION => SkillState::Newer,
            Some(_) => SkillState::Older,
            None => SkillState::Foreign,
        },
        // A folder of that name holding something else is someone else's;
        // an empty one, as a dotfiles checkout may link, holds nothing yet.
        Err(e) if e.kind() == io::ErrorKind::NotFound && holds_anything(path) => {
            SkillState::Foreign
        }
        Err(e) if e.kind() == io::ErrorKind::NotFound => SkillState::Missing,
        Err(e) => SkillState::Unreadable(format!("reading {}: {e}", path.display())),
    }
}

/// Whether a folder holds anything but hidden files such as `.DS_Store`.
fn holds_anything(path: &Path) -> bool {
    fs::read_dir(path).is_ok_and(|entries| {
        entries
            .flatten()
            .any(|e| !e.file_name().as_encoded_bytes().starts_with(b"."))
    })
}

fn write_skill(path: &Path, done: Change, dry_run: bool) -> Change {
    if dry_run {
        return done;
    }
    match agent_files::write(&path.join("SKILL.md"), stub::text().as_bytes()) {
        Ok(()) => done,
        Err(e) => Change::Failed(format!("writing {}: {e}", path.display())),
    }
}

fn replaced(result: io::Result<Replaced>, done: Change) -> Change {
    match result {
        Ok(Replaced::Done) => done,
        Ok(Replaced::ChangedMeanwhile) => {
            Change::Failed("changed by another program meanwhile; run setup again".into())
        }
        Err(e) => Change::Failed(e.to_string()),
    }
}

/// Removes the stub, and its folder when that held nothing else; a linked
/// folder stays, as its link's owner keeps it.
fn remove_skill(path: &Path) -> io::Result<()> {
    fs::remove_file(path.join("SKILL.md"))?;
    let linked = fs::symlink_metadata(path)?.file_type().is_symlink();
    if !linked && !holds_anything(path) {
        fs::remove_dir_all(path)?;
    }
    Ok(())
}
