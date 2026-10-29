# Rhythm

Rhythm shows the background jobs agents leave on your machines and which of
them need attention. launchd keeps running them; Rhythm reads what launchd
knows, records each run of the jobs that opt in, and makes it one command away
for you and your agents.

```sh
rhythm status                    # every user launchd job, attention first
rhythm status --host devbox      # another machine over ssh
rhythm history com.example.scan  # recorded runs, newest first
rhythm logs com.example.scan     # output of the newest run
rhythm --json status             # structured output for agents
```

See [VISION.md](VISION.md) for scope, [status](docs/status.md) for states and
the JSON contract, [recorded runs](docs/runs.md) for history, output, and
retention, and [distribution](docs/distribution.md) for signed
installation. Real job definitions and output belong in private storage, outside
this repository.

## Install and run

Versioned releases use mise's packslip backend to verify the signed release and
archive. See [distribution](docs/distribution.md).

For development, install the pinned tools in `mise.toml`, then:

```sh
cargo build --locked
target/debug/rhythm status
```

Commands print text, or JSON with `--json`. A request that cannot run at all
fails with `{"error":{"code":…,"message":…}}` on stderr. A host that cannot be
read is reported inside the status response on stdout. Exit codes are `0` for
success and `1` for any failure, including one unreachable host.

## Verification

```sh
mise run check
cargo test --locked
python distribution/smoke.py --binary target/debug/rhythm --version 0.1.1
# Optional checks that register disposable agents with the real user launchd:
cargo test --locked --test launchd_e2e -- --ignored
python distribution/smoke.py --binary target/debug/rhythm --version 0.1.1 --launchd
```

Tests use synthetic definitions, a fake `launchctl` and `ssh` at the process
boundary, real recorder processes with temporary state, and golden files for
terminal output. Submit reviewed changes through
mergegate.
