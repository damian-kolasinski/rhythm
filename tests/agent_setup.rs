#![cfg(unix)]
use rhythm::agent_setup::{self, Agent, Change, HookState, Kind, Scope, SkillState, hook_command};
use serde_json::{Value, json};
use std::{fs, os::unix::fs::PermissionsExt, path::PathBuf};
use tempfile::TempDir;

struct Home {
    temp: TempDir,
}
impl Home {
    fn new(agents: &[&str]) -> Self {
        let temp = TempDir::new().unwrap();
        for dir in agents {
            fs::create_dir_all(temp.path().join(dir)).unwrap();
        }
        Self { temp }
    }
    fn path(&self, relative: &str) -> PathBuf {
        self.temp.path().join(relative)
    }
    fn scope(&self) -> Scope {
        Scope::user(self.temp.path().to_owned())
    }
    fn json(&self, relative: &str) -> Value {
        serde_json::from_str(&fs::read_to_string(self.path(relative)).unwrap()).unwrap()
    }
    fn write(&self, relative: &str, text: &str) {
        let path = self.path(relative);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }
}

fn changes(steps: &[agent_setup::Step]) -> Vec<(Vec<Agent>, Kind, Change)> {
    steps
        .iter()
        .map(|s| (s.agents.clone(), s.kind, s.change.clone()))
        .collect()
}

fn hooks_of(settings: &Value) -> Vec<String> {
    settings
        .pointer("/hooks/SessionStart")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .flat_map(|g| g["hooks"].as_array().unwrap().clone())
        .map(|h| h["command"].as_str().unwrap().to_owned())
        .collect()
}

#[test]
fn sets_up_only_the_agents_found_and_shares_folders() {
    let home = Home::new(&[".claude", ".config/opencode"]);
    let scope = home.scope();
    assert_eq!(scope.found(), [Agent::Claude, Agent::OpenCode]);
    let steps = agent_setup::setup(&scope, &scope.found(), false, false);
    assert_eq!(
        changes(&steps),
        [
            (vec![Agent::Claude], Kind::Skill, Change::Installed),
            (vec![Agent::OpenCode], Kind::Skill, Change::Installed),
            (vec![Agent::Claude], Kind::Hook, Change::Installed),
            (vec![Agent::OpenCode], Kind::Hook, Change::Unsupported),
        ]
    );
    assert!(home.path(".agents/skills/rhythm/SKILL.md").exists());
    assert!(!home.path(".codex").exists());
    assert_eq!(
        hooks_of(&home.json(".claude/settings.json")),
        [hook_command(Agent::Claude)]
    );
}

#[test]
fn never_replaces_a_skill_rhythm_did_not_write_unless_forced() {
    let home = Home::new(&[".claude"]);
    let own = "---\nname: rhythm\ndescription: the operator's notes\n---\nMine.\n";
    home.write(".claude/skills/rhythm/SKILL.md", own);
    let steps = agent_setup::setup(&home.scope(), &[Agent::Claude], false, false);
    assert_eq!(steps[0].change, Change::Conflict);
    assert!(steps[0].change.failed());
    assert_eq!(
        fs::read_to_string(home.path(".claude/skills/rhythm/SKILL.md")).unwrap(),
        own
    );
    let removed = agent_setup::remove(&home.scope(), &[Agent::Claude], false);
    assert_eq!(removed[0].change, Change::Kept);
    let forced = agent_setup::setup(&home.scope(), &[Agent::Claude], true, false);
    assert_eq!(forced[0].change, Change::Updated);
}

#[test]
fn refresh_updates_only_what_an_older_rhythm_wrote() {
    let home = Home::new(&[".claude", ".codex"]);
    let scope = home.scope();
    agent_setup::setup(&scope, &[Agent::Claude], false, false);
    // An older stub and an older spelling of the hook.
    let stub = home.path(".claude/skills/rhythm/SKILL.md");
    let current = fs::read_to_string(&stub).unwrap();
    fs::write(
        &stub,
        current.replace("rhythm-stub: \"1\"", "rhythm-stub: \"0\""),
    )
    .unwrap();
    home.write(
        ".claude/settings.json",
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"rhythm skill session-start --agent claude"}]}]}}"#,
    );
    let steps = agent_setup::refresh(&scope);
    assert_eq!(
        changes(&steps),
        [
            (vec![Agent::Claude], Kind::Skill, Change::Updated),
            (vec![Agent::Claude], Kind::Hook, Change::Updated),
        ]
    );
    assert_eq!(fs::read_to_string(&stub).unwrap(), current);
    assert_eq!(
        hooks_of(&home.json(".claude/settings.json")),
        [hook_command(Agent::Claude)]
    );
    // Nothing new is installed, even for an agent that was found.
    assert!(!home.path(".agents/skills/rhythm").exists());
    assert!(!home.path(".codex/hooks.json").exists());
    assert!(agent_setup::refresh(&scope).is_empty());
}

#[test]
fn a_dry_run_changes_nothing() {
    let home = Home::new(&[".claude", ".codex"]);
    let steps = agent_setup::setup(&home.scope(), &home.scope().found(), false, true);
    assert!(steps.iter().any(|s| s.change == Change::Installed));
    assert!(!home.path(".claude/skills").exists());
    assert!(!home.path(".claude/settings.json").exists());
    assert!(!home.path(".codex/hooks.json").exists());
}

#[test]
fn leaves_settings_it_cannot_read_untouched_and_goes_ahead_with_the_rest() {
    let home = Home::new(&[".claude", ".codex"]);
    home.write(".claude/settings.json", "{ not json");
    home.write(".codex/hooks.json", r#"{"hooks": []}"#);
    let steps = agent_setup::setup(&home.scope(), &home.scope().found(), false, false);
    let hooks: Vec<&Change> = steps
        .iter()
        .filter(|s| s.kind == Kind::Hook)
        .map(|s| &s.change)
        .collect();
    assert!(matches!(hooks[..], [Change::Failed(_), Change::Failed(_)]));
    assert_eq!(
        fs::read_to_string(home.path(".claude/settings.json")).unwrap(),
        "{ not json"
    );
    assert!(home.path(".claude/skills/rhythm/SKILL.md").exists());
}

#[test]
fn keeps_links_permissions_and_other_tools_hooks() {
    let home = Home::new(&[".codex"]);
    let real = home.path("dotfiles/codex-hooks.json");
    home.write(
        "dotfiles/codex-hooks.json",
        &json!({
            "hooks": {"SessionStart": [{"hooks": [{"type": "command", "command": "other-tool"}]}]}
        })
        .to_string(),
    );
    fs::set_permissions(&real, fs::Permissions::from_mode(0o600)).unwrap();
    std::os::unix::fs::symlink(&real, home.path(".codex/hooks.json")).unwrap();
    let steps = agent_setup::setup(&home.scope(), &[Agent::Codex], false, false);
    assert_eq!(
        steps[1].approval(),
        Some("trust the new hook with /hooks in Codex")
    );
    assert!(
        fs::symlink_metadata(home.path(".codex/hooks.json"))
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        fs::metadata(&real).unwrap().permissions().mode() & 0o777,
        0o600
    );
    assert_eq!(
        hooks_of(&home.json(".codex/hooks.json")),
        ["other-tool".to_owned(), hook_command(Agent::Codex)]
    );
    agent_setup::remove(&home.scope(), &[Agent::Codex], false);
    assert_eq!(hooks_of(&home.json(".codex/hooks.json")), ["other-tool"]);
}

#[test]
fn removing_from_a_shared_folder_says_who_loses_it_and_keeps_linked_folders() {
    let home = Home::new(&[".codex", ".config/opencode", ".claude"]);
    let elsewhere = home.path("dotfiles/rhythm-skill");
    fs::create_dir_all(&elsewhere).unwrap();
    fs::create_dir_all(home.path(".claude/skills")).unwrap();
    std::os::unix::fs::symlink(&elsewhere, home.path(".claude/skills/rhythm")).unwrap();
    agent_setup::setup(&home.scope(), &home.scope().found(), false, false);
    assert!(elsewhere.join("SKILL.md").exists());
    let steps = agent_setup::remove(&home.scope(), &[Agent::Codex, Agent::Claude], false);
    assert_eq!(steps[0].agents, [Agent::Claude]);
    assert_eq!(steps[1].agents, [Agent::Codex, Agent::OpenCode]);
    assert!(!home.path(".agents/skills/rhythm").exists());
    // The link's owner keeps the folder it points at.
    assert!(home.path(".claude/skills/rhythm").exists());
    assert!(!elsewhere.join("SKILL.md").exists());
}

#[test]
fn status_reports_each_agent() {
    let home = Home::new(&[".claude"]);
    agent_setup::setup(&home.scope(), &[Agent::Claude], false, false);
    let status = agent_setup::status(&home.scope());
    assert!(status[0].found && status[0].ready());
    assert_eq!(status[1].skill_state, SkillState::Missing);
    assert!(!status[1].found);
    assert_eq!(status[2].hook_state, HookState::Unsupported);
}

#[test]
fn the_session_hook_never_fails_or_speaks_without_news() {
    let home = Home::new(&[".claude"]);
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .args(["--json", "skill", "session-start", "--agent", "claude"])
        .env("HOME", home.temp.path())
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(
        output.stdout.is_empty(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    // Even where Rhythm cannot work at all.
    let output = std::process::Command::new(env!("CARGO_BIN_EXE_rhythm"))
        .args(["skill", "session-start", "--agent", "codex"])
        .env("HOME", "relative")
        .output()
        .unwrap();
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
}

#[test]
fn an_older_rhythm_never_replaces_a_newer_stub() {
    let home = Home::new(&[".claude"]);
    agent_setup::setup(&home.scope(), &[Agent::Claude], false, false);
    let stub = home.path(".claude/skills/rhythm/SKILL.md");
    let newer = fs::read_to_string(&stub)
        .unwrap()
        .replace("rhythm-stub: \"1\"", "rhythm-stub: \"9\"");
    fs::write(&stub, &newer).unwrap();
    assert!(agent_setup::refresh(&home.scope()).is_empty());
    assert_eq!(fs::read_to_string(&stub).unwrap(), newer);
    assert!(agent_setup::status(&home.scope())[0].ready());
}

#[test]
fn hidden_files_do_not_make_a_folder_someone_elses() {
    let home = Home::new(&[".claude"]);
    home.write(".claude/skills/rhythm/.DS_Store", "finder");
    let steps = agent_setup::setup(&home.scope(), &[Agent::Claude], false, false);
    assert_eq!(steps[0].change, Change::Installed);
}

#[test]
fn keys_the_operator_added_to_the_hook_survive_refreshes() {
    let home = Home::new(&[".claude"]);
    agent_setup::setup(&home.scope(), &[Agent::Claude], false, false);
    let mut settings = home.json(".claude/settings.json");
    settings["hooks"]["SessionStart"][0]["matcher"] = json!("");
    settings["hooks"]["SessionStart"][0]["hooks"][0]["timeout"] = json!(10);
    home.write(".claude/settings.json", &settings.to_string());
    assert!(agent_setup::refresh(&home.scope()).is_empty());
    assert_eq!(home.json(".claude/settings.json"), settings);
}

fn rhythm(home: &Home, args: &[&str], env: &[(&str, &str)]) -> std::process::Output {
    let mut command = std::process::Command::new(env!("CARGO_BIN_EXE_rhythm"));
    command.args(args).env("HOME", home.temp.path());
    for (key, value) in env {
        command.env(key, value);
    }
    command.output().unwrap()
}

#[test]
fn ordinary_commands_never_touch_agent_files() {
    let home = Home::new(&[".claude"]);
    home.write(
        ".claude/settings.json",
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"rhythm skill session-start --agent claude"}]}]}}"#,
    );
    let before = fs::read_to_string(home.path(".claude/settings.json")).unwrap();
    for args in [&["status"][..], &["attention"]] {
        rhythm(&home, args, &[]);
    }
    assert_eq!(
        fs::read_to_string(home.path(".claude/settings.json")).unwrap(),
        before
    );
    // The session hook is where an older setup is brought up to date.
    rhythm(&home, &["skill", "session-start", "--agent", "claude"], &[]);
    assert_eq!(
        hooks_of(&home.json(".claude/settings.json")),
        [hook_command(Agent::Claude)]
    );
}

#[test]
fn inside_a_job_the_session_hook_stays_out_of_the_way() {
    let home = Home::new(&[".claude"]);
    home.write(
        ".claude/settings.json",
        r#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"rhythm skill session-start --agent claude"}]}]}}"#,
    );
    let before = fs::read_to_string(home.path(".claude/settings.json")).unwrap();
    let output = rhythm(
        &home,
        &["skill", "session-start", "--agent", "claude"],
        &[("RHYTHM_JOB", "dev.rhythm.sample")],
    );
    assert!(output.status.success() && output.stdout.is_empty());
    // Not even a refresh, and no look is recorded.
    assert_eq!(
        fs::read_to_string(home.path(".claude/settings.json")).unwrap(),
        before
    );
    assert!(
        !home
            .path("Library/Application Support/Rhythm/attention")
            .exists()
    );
}
