# Sessions started by a routine

A routine can start an agent in a fresh worktree. Rhythm records the launch
with the run that requested it and keeps the session record independently of
run output. The agent can report an outcome after the routine exits.

## Choose a launcher

The `default` profile suggests Claude Code in the background. It uses
Claude's default model and auto permission mode. Configure an absolute
launcher path when it is outside launchd's PATH. The same CLI commands are
available to an operator or an agent setting up a routine.

```sh
rhythm session configure review --backend claude
rhythm session configure repair --backend t3 --executable /absolute/path/to/t3
rhythm session profile repair
```

T3 uses a running local T3 Code environment and starts a top-level thread in
its own worktree through `t3 thread start`. Register the checkout as a T3
project first. The repository path selects the project by default. Pass
`--project` to select a project explicitly, or `--t3-data-dir` for a different
local environment. T3 uses the project's model and permission mode. Pass
`--provider` and `--model` together to override its model. Claude also accepts
a `--model` override.

## Start a session

```sh
rhythm session spawn --profile review --key review-2026-01-01 \
  --title 'Review open entries' --repo /absolute/path/to/project \
  --base origin/main < prompt.txt
```

Use one stable key per unit of work. A confirmed retry in the same routine
returns the original Rhythm session ID, and the original run keeps credit
for the launch. A manual invocation scopes its keys to the repository.
Refresh the desired ref before asking Rhythm to launch. Use `--add-dir`
(repeatable) to give Claude access to additional directories. T3 receives
those directory paths as context and applies the project's permission mode.

The command prints the Rhythm session ID. With `--json` it also returns the
provider ID, branch, and provider link when available. If the launcher runs
but its reply cannot confirm creation, the command returns
`session_launch_unconfirmed`. The key stays reserved to avoid creating a
duplicate. `rhythm session list --run ID --include-unconfirmed` shows the
reservation. After verifying that the provider started no session, use
`rhythm session forget-unconfirmed SESSION_ID` to release the key. This is
available five minutes after the attempt.

Run history shows `session_spawns.count` for runs that started sessions.
`rhythm session list --run ID` returns the individual sessions.
`rhythm session show SESSION_ID` adds the provider's current activity when
it is available. A routine can use that activity to decide when to launch
another session.

## Report what happened

Rhythm appends the outcome-reporting command to the agent's prompt. Reports
can be replaced as work progresses. To report a PR later, update the summary
and pass its link again with `--pr`. Multiple PRs use repeatable `--pr` flags.

```sh
rhythm session report SESSION_ID --state done --summary 'Tests pass. PR ready.' \
  --pr https://example.com/owner/project/pull/42
```

Use `waiting` when the task needs a decision or review, and `failed` when the
task could not be completed. A provider finishing an agent turn and the
agent reporting a task outcome are tracked independently. Prompts and
provider transcripts are omitted from Rhythm's session records. Keep secrets
out of titles and reports.
