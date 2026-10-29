# Rhythm

Rhythm shows the background jobs agents leave on your machines, records what
each run did, and tells you and your next agent session when one breaks.
launchd keeps running the jobs; Rhythm makes them visible and predictable.

```sh
rhythm demo                      # a two-minute tour with disposable jobs
rhythm status                    # every user launchd job, attention first
rhythm status --host devbox      # another machine over ssh
rhythm add report --daily 08:07 -- /path/to/report   # dev.rhythm.report
rhythm kick report --wait        # run now and see how it ended
rhythm adopt com.example.scan    # record an existing job's runs
rhythm history report            # recorded runs, newest first
rhythm logs report               # output of the newest run
rhythm attention --since-last-seen --as claude  # what broke since last look
rhythm skill setup               # teach Claude Code, Codex, and OpenCode to use Rhythm
```

- [Status](docs/status.md): states, other machines, and the JSON contract.
- [Recorded runs](docs/runs.md): the recorder, history, output, and retention.
- [Creating and adopting jobs](docs/authoring.md): `add`, `adopt`, `remove`, `kick`.
- [Attention and notifications](docs/attention.md): failure notifications and
  what broke since an agent last looked.
- [Setting agents up](docs/agents.md): the skill and session hook `rhythm skill
  setup` installs, and what it leaves alone.
- [Distribution](docs/distribution.md): signed installation and releases.

See [VISION.md](VISION.md) for scope. Real job definitions and output belong in
private storage, outside this repository.

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
python distribution/smoke.py --binary target/debug/rhythm --version 0.2.0
# Optional checks that register disposable agents with the real user launchd:
cargo test --locked --test launchd_e2e -- --ignored
python distribution/smoke.py --binary target/debug/rhythm --version 0.2.0 --launchd
target/debug/rhythm demo
```

Tests use synthetic definitions, a fake `launchctl` and `ssh` at the process
boundary, real recorder processes with temporary state, and golden files for
terminal output. Submit reviewed changes through
mergegate.
