//! Local process adapters for background Claude sessions and T3 threads.
use super::{Activity, Launch, LaunchFailure, Launcher, Profile, Request, SessionId, web_link};
use crate::{
    authoring,
    bounded_command::{self, Failure},
    operation_error::Error,
    private_storage,
};
use serde::Deserialize;
use std::{
    io::{Seek, Write},
    path::{Path, PathBuf},
    process::{Command, Stdio},
    time::Duration,
};

const LAUNCH_TIMEOUT: Duration = Duration::from_secs(120);
const STATUS_TIMEOUT: Duration = Duration::from_secs(5);
const OUTPUT_LIMIT: u64 = 512 * 1024;

pub struct LocalLaunchers<'a> {
    pub state_dir: &'a Path,
}

impl Launcher for LocalLaunchers<'_> {
    fn resolve(&self, mut profile: Profile) -> Profile {
        let (path, name) = match &mut profile {
            Profile::Claude { executable, .. } => (executable, "claude"),
            Profile::T3 { executable, .. } => (executable, "t3"),
        };
        if path.is_none() {
            *path = find_launcher(name);
        }
        profile
    }

    fn start(
        &self,
        profile: &Profile,
        request: &Request,
        id: &SessionId,
        prompt: &str,
    ) -> Result<Launch, LaunchFailure> {
        match profile {
            Profile::Claude { executable, model } => {
                let branch = format!("rhythm/session-{}", id.as_str());
                let worktree = self.worktree(request, id, &branch)?;
                let mut command = claude(executable.as_deref());
                command.current_dir(worktree).args([
                    "--bg",
                    "--name",
                    &request.title,
                    "--permission-mode",
                    "auto",
                ]);
                if let Some(model) = model {
                    command.args(["--model", model]);
                }
                command.arg(format!("Task\n\n{prompt}"));
                if !request.add_dirs.is_empty() {
                    command.arg("--add-dir").args(&request.add_dirs);
                }
                let output = start(command, false)?;
                let provider_id = output
                    .stdout
                    .split(|byte| *byte == b'\n')
                    .chain(output.stderr.split(|byte| *byte == b'\n'))
                    .filter_map(|line| std::str::from_utf8(line).ok())
                    .filter_map(claude_id)
                    .next()
                    .ok_or(LaunchFailure::Unconfirmed)?;
                Ok(Launch {
                    provider_id: provider_id.into(),
                    link: None,
                    branch: Some(branch),
                })
            }
            Profile::T3 {
                executable,
                project,
                data_dir,
                model,
            } => {
                let branch = format!("rhythm/session-{}", id.as_str());
                let mut command = t3(executable.as_deref(), data_dir.as_deref(), "start");
                command.current_dir(&request.repository).args([
                    "--json",
                    "--title",
                    &request.title,
                    "--worktree",
                    &request.base,
                    "--branch",
                    &branch,
                ]);
                if let Some(model) = model {
                    command.args(["--provider", &model.provider, "--model", &model.name]);
                }
                if let Some(project) = project {
                    command.arg(project);
                } else {
                    command.arg(&request.repository);
                }
                command.arg("-");
                let mut input = tempfile::tempfile().map_err(|_| before_storage())?;
                input
                    .write_all(prompt.as_bytes())
                    .map_err(|_| before_storage())?;
                for dir in &request.add_dirs {
                    writeln!(input, "\nAdditional context directory: {}", dir.display())
                        .map_err(|_| before_storage())?;
                }
                input.rewind().map_err(|_| before_storage())?;
                command.stdin(Stdio::from(input));
                let output = start(command, true)?;
                let reply: Thread = serde_json::from_slice(&output.stdout)
                    .map_err(|_| LaunchFailure::Unconfirmed)?;
                let provider_id = reply
                    .thread_id
                    .filter(|id| provider_id(id))
                    .ok_or(LaunchFailure::Unconfirmed)?;
                let link = reply.link.filter(|link| web_link(link) || t3_link(link));
                Ok(Launch {
                    provider_id,
                    link,
                    branch: Some(branch),
                })
            }
        }
    }

    fn observe(&self, profile: &Profile, id: &SessionId, launch: &mut Launch) -> Activity {
        match profile {
            Profile::Claude { executable, .. } => {
                let mut command = claude(executable.as_deref());
                let worktree = self.state_dir.join("worktrees").join(id.as_str());
                command.args(["agents", "--json", "--cwd"]).arg(worktree);
                let Some(agents): Option<Vec<ClaudeAgent>> = read(command) else {
                    return Activity::Unknown;
                };
                agents
                    .iter()
                    .find(|agent| {
                        agent.id.starts_with(&launch.provider_id)
                            || agent.session_id.starts_with(&launch.provider_id)
                    })
                    .map_or(Activity::Missing, |agent| activity(&agent.status))
            }
            Profile::T3 {
                executable,
                data_dir,
                ..
            } => {
                let mut command = t3(executable.as_deref(), data_dir.as_deref(), "show");
                command.args(["--json", "--messages", "0", &launch.provider_id]);
                read::<ThreadShow>(command).map_or(Activity::Unknown, |reply| {
                    if launch.link.is_none() {
                        launch.link = reply
                            .thread
                            .link
                            .filter(|link| web_link(link) || t3_link(link));
                    }
                    if reply.thread.archived || reply.thread.settled {
                        Activity::Ended
                    } else if reply.thread.pending_request_count.unwrap_or(0) > 0 {
                        Activity::Waiting
                    } else {
                        activity(reply.thread.status.as_deref().unwrap_or_default())
                    }
                })
            }
        }
    }
}

impl LocalLaunchers<'_> {
    fn worktree(
        &self,
        request: &Request,
        id: &SessionId,
        branch: &str,
    ) -> Result<PathBuf, LaunchFailure> {
        let root = self.state_dir.join("worktrees");
        private_storage::private_dir(&root).map_err(LaunchFailure::BeforeLaunch)?;
        let worktree = root.join(id.as_str());
        let mut resolve = Command::new("git");
        resolve.current_dir(&request.repository).args([
            "rev-parse",
            "--verify",
            "--end-of-options",
            &format!("{}^{{commit}}", request.base),
        ]);
        let commit = bounded_command::run(resolve, STATUS_TIMEOUT, 1024)
            .ok()
            .filter(|output| output.status.success())
            .and_then(|output| String::from_utf8(output.stdout).ok())
            .map(|commit| commit.trim().to_owned())
            .filter(|commit| {
                matches!(commit.len(), 40 | 64) && commit.bytes().all(|b| b.is_ascii_hexdigit())
            })
            .ok_or(LaunchFailure::BeforeLaunch(Error::SessionWorkspaceFailed))?;
        let mut git = Command::new("git");
        git.current_dir(&request.repository)
            .args(["worktree", "add", "-q", "-b", branch])
            .arg(&worktree)
            .arg(commit);
        let added = bounded_command::run(git, LAUNCH_TIMEOUT, OUTPUT_LIMIT)
            .is_ok_and(|output| output.status.success());
        if added {
            Ok(worktree)
        } else {
            Err(LaunchFailure::BeforeLaunch(Error::SessionWorkspaceFailed))
        }
    }
}

fn claude(executable: Option<&Path>) -> Command {
    launcher(executable, "claude")
}

fn t3(executable: Option<&Path>, data_dir: Option<&Path>, action: &str) -> Command {
    let mut command = launcher(executable, "t3");
    command.args(["thread", action]);
    if let Some(data_dir) = data_dir {
        command.arg("--base-dir").arg(data_dir);
    }
    command
}

fn launcher(executable: Option<&Path>, fallback: &str) -> Command {
    let mut command = Command::new(executable.unwrap_or_else(|| Path::new(fallback)));
    for (name, _) in std::env::vars_os() {
        let name_text = name.to_string_lossy();
        if name_text == "CLAUDECODE"
            || name_text == "T3CODE_HOME"
            || name_text.starts_with("CLAUDE_CODE_")
            || name_text.starts_with("CODEX_")
            || name_text.starts_with("T3CODE_")
        {
            command.env_remove(name);
        }
    }
    command
        .env_remove(super::RUN_ID)
        .env_remove(super::DATA_DIR)
        .env_remove(crate::recorder::JOB)
        .env_remove(crate::run_result::VARIABLE);
    command.stdin(Stdio::null());
    command
}

fn find_launcher(name: &str) -> Option<PathBuf> {
    let home = std::env::var_os("HOME").map(PathBuf::from);
    let default = home
        .as_deref()
        .and_then(|home| authoring::default_path(home).ok())
        .unwrap_or_default();
    let path = std::env::var_os("PATH").unwrap_or_default();
    let t3 = home
        .filter(|_| name == "t3")
        .map(|home| home.join(".t3/bin"));
    t3.into_iter()
        .chain(std::env::split_paths(&default))
        .chain(std::env::split_paths(&path))
        .filter(|path| path.is_absolute())
        .map(|path| path.join(name))
        .find(|path| authoring::is_executable(path))
}

fn start(command: Command, t3: bool) -> Result<bounded_command::Output, LaunchFailure> {
    match bounded_command::run_prepared(command, LAUNCH_TIMEOUT, OUTPUT_LIMIT) {
        Ok(output) if output.status.success() => Ok(output),
        Ok(output) if t3 && t3_rejected(&output.stderr) => {
            Err(LaunchFailure::BeforeLaunch(Error::SessionLauncherRejected))
        }
        Err(Failure::Unavailable) => Err(LaunchFailure::BeforeLaunch(
            Error::SessionLauncherUnavailable,
        )),
        _ => Err(LaunchFailure::Unconfirmed),
    }
}

fn t3_rejected(stderr: &[u8]) -> bool {
    String::from_utf8_lossy(stderr).lines().any(|line| {
        line.starts_with("No running T3 Code server uses ")
            || line.starts_with("No T3 Code project matches ")
            || line.starts_with("This T3 Code server has no t3_thread_launch tool.")
            || line.starts_with("The server refused the arguments:")
    })
}

fn read<T: serde::de::DeserializeOwned>(command: Command) -> Option<T> {
    let output = bounded_command::run(command, STATUS_TIMEOUT, OUTPUT_LIMIT).ok()?;
    output
        .status
        .success()
        .then(|| serde_json::from_slice(&output.stdout).ok())
        .flatten()
}

fn before_storage() -> LaunchFailure {
    LaunchFailure::BeforeLaunch(Error::StorageUnavailable)
}

fn claude_id(line: &str) -> Option<&str> {
    let mut words = line.split_whitespace();
    if words.next()? != "backgrounded" {
        return None;
    }
    words.next()?;
    words.next().filter(|id| provider_id(id))
}

fn provider_id(id: &str) -> bool {
    (6..=128).contains(&id.len())
        && id
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
}

fn t3_link(link: &str) -> bool {
    link.strip_prefix("t3-thread://").is_some_and(|path| {
        !path.is_empty()
            && path.len() < 400
            && path
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b"/-_".contains(&b))
    })
}

fn activity(status: &str) -> Activity {
    match status {
        "preparing" | "queued" | "starting" | "running" | "working" => Activity::Running,
        "blocked" | "waiting" | "awaiting_input" => Activity::Waiting,
        "idle" | "completed" | "done" => Activity::Idle,
        "failed" | "cancelled" | "rolled_back" | "interrupted" | "stopped" | "exited" => {
            Activity::Ended
        }
        _ => Activity::Unknown,
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Thread {
    thread_id: Option<String>,
    link: Option<String>,
    status: Option<String>,
    pending_request_count: Option<u32>,
    #[serde(default)]
    archived: bool,
    #[serde(default)]
    settled: bool,
}

#[derive(Deserialize)]
struct ThreadShow {
    thread: Thread,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct ClaudeAgent {
    #[serde(default)]
    id: String,
    #[serde(default)]
    session_id: String,
    #[serde(alias = "state", default)]
    status: String,
}
