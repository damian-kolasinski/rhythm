# Rhythm

Rhythm runs recurring commands on one machine and keeps their next run, result,
history, and output available across sessions. Define interval, daily, or weekly
schedules through the CLI, then inspect and control them in a small local web UI.

See [VISION.md](VISION.md) for scope, [schedule behavior](docs/schedules.md),
[execution and storage](docs/execution.md), [runtime supervision](docs/runtime.md),
and [browser behavior](docs/web-ui.md). Real definitions and execution output
belong in private operational storage, outside this repository.

## Install and run

Versioned releases use mise's packslip backend to verify the signed release and
archive. See [distribution](docs/distribution.md) for installation and verification.

For development, install the pinned tools in `mise.toml`, then:

```sh
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo build --locked
target/debug/rhythm runtime run
```

In another terminal, create a schedule from a private definition file. This
example uses only synthetic data:

```json
{
  "id": "sample-report",
  "command": ["/bin/sh", "-c", "printf 'sample report'"],
  "working_directory": "/tmp",
  "trigger": { "type": "interval", "every_ms": 60000 },
  "overlap": "skip",
  "missed": "run_once"
}
```

```sh
target/debug/rhythm --json schedule create --file /tmp/sample-schedule.json
target/debug/rhythm --json schedule run-now sample-report
target/debug/rhythm --json schedule history sample-report
target/debug/rhythm --json schedule logs 1
target/debug/rhythm --json schedule resume sample-report
target/debug/rhythm ui
```

Schedules start paused. `run-now` runs while paused; `resume` enables automatic
triggers. `pause` allows active and accepted manual runs to finish. `cancel`
stops active and queued runs, leaving future triggers enabled unless paused.

Successful commands return JSON. `--json` also makes errors structured. Exit
codes are `0` for success and `1` for failure. Commands require absolute executable
and working-directory paths, receive a fixed system PATH, and inherit no session
environment or credentials. A shell runs only when explicitly configured.

Use `runtime install` on macOS to supervise the scheduler with launchd, or
`runtime stop` to stop it. `runtime uninstall` preserves private state and logs.
Use an executable at a stable location. See [runtime](docs/runtime.md).

The schedules-only database uses schema 3. Other existing schema versions are
refused with `unsupported_schema`; start with a fresh `--data-dir` when switching
from a different schema. There is no migration or compatibility layer.

## Demo and verification

`rhythm demo` opens disposable synthetic schedules using the real runtime.
Ctrl-C stops execution and removes the temporary state. `--no-open` prints the
URL for manual browser opening. The local browser trusts callers that can reach
loopback, including other local OS accounts; see [web trust](docs/web-ui.md).

```sh
mise run check
cargo test --locked
cargo build --locked
pnpm --dir web exec playwright install chromium
pnpm --dir web test
python distribution/smoke.py --binary target/debug/rhythm --version 0.1.1
# Optional macOS launchd check using disposable state:
cargo test --locked --test schedules_e2e launchd_runtime_executes_and_restores_persisted_schedules_end_to_end -- --ignored
```

Checks use synthetic fixtures and cover recurrence/DST boundaries, overlap and
missed-run policies, durable dispatch, runtime interruption, uncertain outcomes,
contained cancellation, private storage, API authorization, logs, and browser
flows/rendering. Submit reviewed changes through mergegate.
