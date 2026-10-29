//! The coding agents `rhythm skill setup` knows, and where each reads skills
//! and session hooks, relative to the user's home.
use serde::Serialize;
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Agent {
    Claude,
    Codex,
    #[serde(rename = "opencode")]
    OpenCode,
}

impl Agent {
    pub const ALL: [Agent; 3] = [Agent::Claude, Agent::Codex, Agent::OpenCode];

    /// As `--agent` takes it, the hook passes it, and attention names its consumer.
    pub fn id(self) -> &'static str {
        match self {
            Agent::Claude => "claude",
            Agent::Codex => "codex",
            Agent::OpenCode => "opencode",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Agent::Claude => "Claude Code",
            Agent::Codex => "Codex",
            Agent::OpenCode => "OpenCode",
        }
    }

    /// Whether it is installed, by the folder it keeps its settings in.
    pub fn found(self, home: &Path) -> bool {
        let config = match self {
            Agent::Claude => ".claude",
            Agent::Codex => ".codex",
            Agent::OpenCode => ".config/opencode",
        };
        home.join(config).is_dir()
    }

    /// The folder it loads skills from. Codex and OpenCode share
    /// `.agents/skills`, which OpenCode reads beside `.claude/skills`.
    pub(super) fn skills(self) -> &'static str {
        match self {
            Agent::Claude => ".claude/skills",
            Agent::Codex | Agent::OpenCode => ".agents/skills",
        }
    }

    /// The file holding its command hooks; `None` when it runs none.
    pub(super) fn hooks(self) -> Option<&'static str> {
        match self {
            Agent::Claude => Some(".claude/settings.json"),
            Agent::Codex => Some(".codex/hooks.json"),
            Agent::OpenCode => None,
        }
    }

    /// What the operator does before a new or changed hook runs: Codex runs
    /// one only once trusted, by its hash.
    pub fn hook_approval(self) -> Option<&'static str> {
        match self {
            Agent::Codex => Some("trust the new hook with /hooks in Codex"),
            Agent::Claude | Agent::OpenCode => None,
        }
    }
}
