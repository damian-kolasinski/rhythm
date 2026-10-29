//! Terminal rendering of agent setup and its status.
use crate::agent_setup::{AgentStatus, Change, HookState, Kind, SkillState, Step};
use std::{fmt::Write, path::Path};

pub fn steps(steps: &[Step], home: &Path) -> String {
    if steps.is_empty() {
        return "No supported agents found: Claude Code, Codex, or OpenCode.\n".into();
    }
    let rows: Vec<[String; 4]> = steps
        .iter()
        .map(|step| {
            let agents: Vec<&str> = step.agents.iter().map(|a| a.label()).collect();
            let kind = match step.kind {
                Kind::Skill => "skill",
                Kind::Hook => "hook",
            };
            let change = match &step.change {
                Change::Installed => "installed".into(),
                Change::Updated => "updated".into(),
                Change::Unchanged => "up to date".into(),
                Change::Removed => "removed".into(),
                Change::Absent => "not there".into(),
                Change::Conflict => {
                    "left alone: a skill named rhythm Rhythm didn't write (--force replaces it)"
                        .into()
                }
                Change::Kept => "left alone: a skill named rhythm Rhythm didn't write".into(),
                Change::Unsupported => "not supported".into(),
                Change::Failed(why) => format!("failed: {why}"),
            };
            let path = step
                .path
                .as_deref()
                .map(|p| short(p, home))
                .unwrap_or_default();
            [agents.join(", "), kind.into(), change, path]
        })
        .collect();
    let mut out = table(&rows);
    for approval in steps.iter().filter_map(Step::approval) {
        writeln!(out, "\nNext: {approval}.").unwrap();
    }
    out
}

pub fn status(agents: &[AgentStatus]) -> String {
    let rows: Vec<[String; 4]> = agents
        .iter()
        .map(|status| {
            [
                status.agent.label().into(),
                if status.found { "found" } else { "not found" }.into(),
                format!("skill {}", skill(&status.skill_state)),
                format!("hook {}", hook(&status.hook_state)),
            ]
        })
        .collect();
    let mut out = table(&rows);
    let pending = agents.iter().filter(|a| a.found && !a.ready()).count();
    if pending > 0 {
        writeln!(
            out,
            "\nRun `rhythm skill setup` to set up {pending} agent{}.",
            if pending == 1 { "" } else { "s" }
        )
        .unwrap();
    }
    out
}

fn skill(state: &SkillState) -> String {
    match state {
        SkillState::Current => "current".into(),
        SkillState::Older => "outdated".into(),
        SkillState::Newer => "from a newer Rhythm".into(),
        SkillState::Missing => "missing".into(),
        SkillState::Foreign => "not Rhythm's".into(),
        SkillState::Unreadable(why) => format!("unreadable ({why})"),
    }
}

fn hook(state: &HookState) -> String {
    match state {
        HookState::Current => "current".into(),
        HookState::Outdated => "outdated".into(),
        HookState::Missing => "missing".into(),
        HookState::Unreadable(why) => format!("unreadable ({why})"),
        HookState::Unsupported => "unsupported".into(),
    }
}

/// `~/…` for paths in the home directory.
fn short(path: &Path, home: &Path) -> String {
    match path.strip_prefix(home) {
        Ok(rest) => format!("~/{}", rest.display()),
        Err(_) => path.display().to_string(),
    }
}

fn table(rows: &[[String; 4]]) -> String {
    let widths: Vec<usize> = (0..3)
        .map(|c| rows.iter().map(|r| r[c].chars().count()).max().unwrap_or(0))
        .collect();
    let mut out = String::new();
    for row in rows {
        let line = format!(
            "{:w0$}  {:w1$}  {:w2$}  {}",
            row[0],
            row[1],
            row[2],
            row[3],
            w0 = widths[0],
            w1 = widths[1],
            w2 = widths[2]
        );
        writeln!(out, "{}", line.trim_end()).unwrap();
    }
    out
}
