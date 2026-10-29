//! The session hook: a `SessionStart` command that has `rhythm skill
//! session-start` tell each new session what broke since that agent last
//! looked. Claude Code and Codex read the same shape: `hooks.SessionStart`
//! holds groups, each with a `hooks` list of handlers.
use super::agent::Agent;
use serde_json::{Map, Value, json};

/// What every command this hook ran contains, so setup finds its own among
/// the operator's.
const MARKER: &str = "rhythm skill session-start";
/// The hook earlier docs had operators add by hand; setup replaces it, so a
/// session is not told the same news twice.
const DOCUMENTED: &str = "rhythm attention --since-last-seen";
/// Seconds an agent gives the hook; `session-start` keeps well within it.
const TIMEOUT: u64 = 5;

/// The command, which must stay byte for byte the same across releases:
/// Codex trusts a hook by its hash. The shell keeps a missing `rhythm` or a
/// failure quiet, so a session starts all the same.
pub fn command(agent: Agent) -> String {
    format!(
        "/bin/sh -c '{MARKER} --agent {} 2>/dev/null || true'",
        agent.id()
    )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HookState {
    Current,
    /// Ours, but not as this Rhythm writes it, or written more than once.
    Outdated,
    Missing,
}

/// Current when exactly one handler of ours runs this Rhythm's command; keys
/// the operator added to it, such as a longer timeout, are theirs to keep.
pub fn state(settings: &Value, agent: Agent) -> HookState {
    let ours: Vec<&Value> = session_start(settings)
        .iter()
        .flat_map(handlers)
        .filter(|h| is_ours(h))
        .collect();
    match ours[..] {
        [] => HookState::Missing,
        [handler] if handler.get("command").and_then(Value::as_str) == Some(&command(agent)) => {
            HookState::Current
        }
        _ => HookState::Outdated,
    }
}

/// Replaces every hook of ours with this Rhythm's, last among the session's
/// hooks. `Err` names what isn't the shape agents read; nothing is changed.
pub fn install(settings: &mut Value, agent: Agent) -> Result<(), &'static str> {
    remove(settings)?;
    let root = settings
        .as_object_mut()
        .ok_or("the settings aren't a JSON object")?;
    let hooks = root
        .entry("hooks")
        .or_insert_with(|| Value::Object(Map::new()))
        .as_object_mut()
        .ok_or("`hooks` isn't an object")?;
    let start = hooks
        .entry("SessionStart")
        .or_insert_with(|| Value::Array(vec![]));
    if start.as_object().is_some_and(Map::is_empty) {
        *start = Value::Array(vec![]);
    }
    match start {
        Value::Array(groups) => groups.push(group(agent)),
        _ => return Err("`hooks.SessionStart` isn't a list"),
    }
    Ok(())
}

/// Takes our handlers out, and the groups, lists, and objects that held only
/// them. Whether anything was ours.
pub fn remove(settings: &mut Value) -> Result<bool, &'static str> {
    let root = settings
        .as_object_mut()
        .ok_or("the settings aren't a JSON object")?;
    let Some(hooks) = root.get_mut("hooks") else {
        return Ok(false);
    };
    let Some(hooks_map) = hooks.as_object_mut() else {
        return Err("`hooks` isn't an object");
    };
    let Some(start) = hooks_map.get_mut("SessionStart") else {
        return Ok(false);
    };
    let Some(groups) = start.as_array() else {
        return match start.as_object().is_some_and(Map::is_empty) {
            true => Ok(false),
            false => Err("`hooks.SessionStart` isn't a list"),
        };
    };
    if !groups.iter().any(has_ours) {
        return Ok(false);
    }
    let kept: Vec<Value> = groups.iter().cloned().filter_map(without_ours).collect();
    if kept.is_empty() {
        hooks_map.shift_remove("SessionStart");
    } else {
        *start = Value::Array(kept);
    }
    if hooks_map.is_empty() {
        root.shift_remove("hooks");
    }
    Ok(true)
}

fn session_start(settings: &Value) -> &[Value] {
    settings
        .pointer("/hooks/SessionStart")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

/// No matcher, so it runs however the session started.
fn group(agent: Agent) -> Value {
    json!({"hooks": [{"type": "command", "command": command(agent), "timeout": TIMEOUT}]})
}

fn handlers(group: &Value) -> &[Value] {
    group
        .get("hooks")
        .and_then(Value::as_array)
        .map_or(&[], Vec::as_slice)
}

fn is_ours(handler: &Value) -> bool {
    handler
        .get("command")
        .and_then(Value::as_str)
        .is_some_and(|command| command.contains(MARKER) || command.contains(DOCUMENTED))
}

fn has_ours(group: &Value) -> bool {
    handlers(group).iter().any(is_ours)
}

/// `group` without our handlers, or `None` when they were all it ran.
fn without_ours(mut group: Value) -> Option<Value> {
    if !has_ours(&group) {
        return Some(group);
    }
    let kept: Vec<Value> = handlers(&group)
        .iter()
        .filter(|h| !is_ours(h))
        .cloned()
        .collect();
    if kept.is_empty() {
        return None;
    }
    group["hooks"] = Value::Array(kept);
    Some(group)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn others() -> Value {
        json!({
            "model": "sample",
            "hooks": {
                "SessionStart": [
                    {"hooks": [{"type": "command", "command": "other-tool session-start", "timeout": 5}]}
                ],
                "Stop": [{"hooks": [{"type": "command", "command": "notify"}]}]
            },
            "permissions": {"allow": []}
        })
    }

    #[test]
    fn installs_last_and_keeps_everything_else_in_order() {
        let mut settings = others();
        assert_eq!(state(&settings, Agent::Claude), HookState::Missing);
        install(&mut settings, Agent::Claude).unwrap();
        assert_eq!(state(&settings, Agent::Claude), HookState::Current);
        let keys: Vec<&String> = settings.as_object().unwrap().keys().collect();
        assert_eq!(keys, ["model", "hooks", "permissions"]);
        let start = settings
            .pointer("/hooks/SessionStart")
            .unwrap()
            .as_array()
            .unwrap();
        assert_eq!(start.len(), 2);
        assert_eq!(start[0], others()["hooks"]["SessionStart"][0]);
        assert_eq!(
            start[1]["hooks"][0]["command"],
            "/bin/sh -c 'rhythm skill session-start --agent claude 2>/dev/null || true'"
        );
        // Installing again changes nothing.
        let before = settings.clone();
        install(&mut settings, Agent::Claude).unwrap();
        assert_eq!(settings, before);
    }

    #[test]
    fn replaces_an_outdated_or_duplicated_hook() {
        let mut settings = others();
        install(&mut settings, Agent::Codex).unwrap();
        assert_eq!(state(&settings, Agent::Claude), HookState::Outdated);
        settings["hooks"]["SessionStart"]
            .as_array_mut()
            .unwrap()
            .push(group(Agent::Claude));
        assert_eq!(state(&settings, Agent::Claude), HookState::Outdated);
        install(&mut settings, Agent::Claude).unwrap();
        assert_eq!(state(&settings, Agent::Claude), HookState::Current);
    }

    #[test]
    fn removes_only_its_own_handlers_and_what_held_only_them() {
        let mut settings = others();
        // A group shared with another tool's handler keeps that handler.
        settings["hooks"]["SessionStart"][0]["hooks"]
            .as_array_mut()
            .unwrap()
            .push(json!({"type": "command", "command": command(Agent::Claude)}));
        install(&mut settings, Agent::Claude).unwrap();
        assert!(remove(&mut settings).unwrap());
        assert_eq!(settings, others());
        assert!(!remove(&mut settings).unwrap());

        let mut only_ours = json!({});
        install(&mut only_ours, Agent::Claude).unwrap();
        remove(&mut only_ours).unwrap();
        assert_eq!(only_ours, json!({}));
    }

    #[test]
    fn keeps_keys_the_operator_added_to_its_handler() {
        let mut settings = json!({});
        install(&mut settings, Agent::Claude).unwrap();
        settings["hooks"]["SessionStart"][0]["matcher"] = json!("");
        settings["hooks"]["SessionStart"][0]["hooks"][0]["timeout"] = json!(10);
        assert_eq!(state(&settings, Agent::Claude), HookState::Current);
    }

    #[test]
    fn replaces_the_hook_earlier_docs_described() {
        let mut settings = json!({"hooks": {"SessionStart": [{"hooks": [
            {"type": "command", "command": "rhythm attention --since-last-seen --as claude-code"}
        ]}]}});
        assert_eq!(state(&settings, Agent::Claude), HookState::Outdated);
        install(&mut settings, Agent::Claude).unwrap();
        assert_eq!(state(&settings, Agent::Claude), HookState::Current);
        assert_eq!(session_start(&settings).len(), 1);
    }

    #[test]
    fn an_empty_session_start_object_becomes_a_list() {
        let mut settings = json!({"hooks": {"SessionStart": {}}});
        install(&mut settings, Agent::Claude).unwrap();
        assert_eq!(state(&settings, Agent::Claude), HookState::Current);
    }

    #[test]
    fn leaves_shapes_it_does_not_understand_alone() {
        for mut settings in [
            json!([]),
            json!({"hooks": []}),
            json!({"hooks": {"SessionStart": {"x": 1}}}),
        ] {
            let before = settings.clone();
            assert!(install(&mut settings, Agent::Claude).is_err(), "{before}");
            assert_eq!(settings, before);
        }
    }
}
