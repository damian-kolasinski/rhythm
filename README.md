# Rhythm

Rhythm keeps background work discoverable, observable, and manageable after
the session that created it ends. See [VISION.md](VISION.md) for the product
direction and [docs/migration.md](docs/migration.md) for the implementation and
cutover plan. Machine inventory is private and must not be committed or published.

The first implementation slice is read-only discovery of the current user's
`~/Library/LaunchAgents` registrations on macOS. Services, scheduling, the
runtime/API, and the web UI are subsequent slices. Discovery does not install,
start, stop, import, or modify any workload.

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

## Verify

```sh
cargo test --locked
cargo clippy --locked --all-targets -- -D warnings
cargo fmt --check
```

Tests cover XML and binary plists, calendar boundaries, interval and startup
triggers, restart policies, observed/unobserved/unknown states, duplicate
labels, partial inventories, secret omission, and terminal control escaping.
On macOS the CLI integration test uses the real `launchctl` with an isolated
HOME and unregistered fixtures; no workload is registered or executed.

## Code review

Use mergegate for code review.
