# Rhythm

Rhythm keeps background work discoverable, observable, and manageable after
the session that created it ends. See [VISION.md](VISION.md) for the product
direction and [docs/migration.md](docs/migration.md) for the implementation and
cutover plan. Machine inventory is private and must not be committed or published.

Rhythm currently provides read-only discovery of the current user's
`~/Library/LaunchAgents` registrations on macOS and a local runtime for
Rhythm-owned services and persisted schedules, with an embedded web workspace.
Discovery does not install, start, stop, import, or modify any workload.

## Run

Install the Rust toolchain specified in `rust-toolchain.toml`, then:

```sh
cargo run -- discover
cargo run -- discover --json
```

JSON has `schema_version: 1`, explicit registration and observation scope and
ownership, registrations, and diagnostics. Exit codes: `0` for a complete
snapshot of this scope, `2` for a partial snapshot, and `1` for an execution/setup
failure. A partial snapshot still includes every successfully parsed
registration. Invalid files produce diagnostics without echoing their contents.
Arguments and environment values are never included; discovery only reports
the number of environment variables. The CLI can run independently on each
machine; it has no remote execution mode.

`launchctl list` observes the calling user's launchd context. Idle registrations
carry their launcher's last exit status; a running PID carries a previous exit
status, which is not a current health check. Neither implies downstream task
completion. A registration absent from that context is `not_observed`, since it
could be loaded in another bootstrap domain (for example, GUI versus SSH).
Unavailable or unrecognized observation produces `unknown` and marks the
snapshot partial.

Discovery excludes system agents/daemons, cron, app-managed schedules and
unregistered processes. An absent LaunchAgents directory is an empty inventory.
Unknown launchd properties are flagged as migration requirements; malformed
registrations are reported separately. No automatic migration is offered.

## Owned services

Build with `cargo build --locked`. Start the shared runtime in one terminal:

```sh
target/debug/rhythm runtime run
```

Create a definition in a private file, using absolute executable and working
directory paths. This example contains only synthetic data:

```json
{
  "id": "sample-worker",
  "command": ["/bin/sh", "-c", "printf 'sample output'"],
  "working_directory": "/tmp",
  "restart": "never",
  "restart_delay_ms": 1000
}
```

Then use another terminal:

```sh
target/debug/rhythm --json service create --file /tmp/sample-service.json
target/debug/rhythm --json service start sample-worker
target/debug/rhythm --json service inspect sample-worker
target/debug/rhythm --json service history sample-worker
target/debug/rhythm --json service logs 1
target/debug/rhythm --json service stop sample-worker
target/debug/rhythm --json runtime stop
```

Service and runtime commands currently return JSON for successful requests,
with `--json` also making failures structured. `service list` finds owned work
across sessions. `service restart` explicitly replaces a current execution;
repeated `start` preserves an existing execution or retry deadline.

On macOS, `runtime install` registers the current executable under the logged-in
user's GUI launchd domain; `runtime status` checks the authenticated API.
Use an executable at a stable location. `runtime stop` removes supervision and
waits for the runtime to release its lock. `runtime uninstall` removes the
registration while preserving state and logs. Versioned distribution follows
in a later slice. See [docs/services.md](docs/services.md) for execution,
recovery, privacy, and API contracts.

## Persisted schedules

Schedules support interval, daily, and weekly triggers with explicit overlap
and missed-run policies. They start paused and share service execution IDs,
private logs, and crash recovery. Use `schedule create --file FILE`, `run-now`,
`resume`, `pause`, `cancel`, `list`, `inspect`, `history`, and `logs`.
See [docs/schedules.md](docs/schedules.md) for definitions, timezone/DST rules,
queueing, recovery, and CLI examples.

## Web workspace and demo

Start the runtime, then run `rhythm ui` to open the workspace directly.
Browse services and schedules, create owned work,
control execution, inspect history and output, and acknowledge investigated
uncertain completions. Like mergegate, the loopback workspace trusts local
callers, including other OS accounts, to create/run commands with the runtime
account's permissions.

Run `rhythm demo` for disposable synthetic work using the real runtime.
Ctrl-C stops the demo and removes its temporary state. Use `--no-open` with
`ui` or `demo` to launch the browser manually. See [docs/web-ui.md](docs/web-ui.md)
for browser trust, build steps, E2E tests, and demo recording instructions.
Versioned mise/packslip distribution and installed-runtime cutover follow next.

## Verify

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
# Rebuild frontend assets before rebuilding the embedded binary
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo build --locked
pnpm --dir web exec playwright install chromium
pnpm --dir web test
# macOS GUI session: installs and removes one disposable registration
cargo test --locked --test services launchd_install_cli_service_and_stop_are_verified_end_to_end -- --ignored
```

Tests cover XML and binary plists, calendar boundaries, interval and startup
triggers, restart policies, observed/unobserved/unknown states, duplicate
labels, partial inventories, secret omission, and terminal control escaping.
On macOS the CLI integration test uses the real `launchctl` with an isolated
HOME and unregistered fixtures; no workload is registered or executed.

Owned-service tests use real processes, SQLite, loopback HTTP, private temporary
storage, and synthetic workloads. They cover cancellation and descendant
cleanup, restart throttling, runtime crashes, uncertain monitor loss, explicit
cleanup acknowledgment, credentials, inherited ACLs, bounded output, and
concurrent shutdown. Schedule tests additionally verify persisted cursors,
calendar dispatch, overlap/missed policies, queueing across interruptions, and
schema migration. Launchd adapter tests fake only the external command.

## Code review

Use mergegate for code review.
