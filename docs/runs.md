# Recorded runs

launchd remembers only a job's last exit code. A job whose definition starts
its command through `rhythm run` also gets a history of runs, with timing,
outcome, and output for each one.

```xml
<key>ProgramArguments</key>
<array>
  <string>/path/to/rhythm</string>
  <string>run</string>
  <string>com.example.scan</string>
  <string>--</string>
  <string>/path/to/scan</string>
  <string>--daily</string>
</array>
```

The label after `run` must match the job's `Label` for `status` to show its
runs, and the definition must use `ProgramArguments` without `Program`. A job
may name its own state directory with `--data-dir ABSOLUTE_PATH` before `run`;
`status` reads each job's runs from the directory its recorder writes to. Use
an executable at a stable location, such as a pinned mise install.

```sh
rhythm history com.example.scan              # newest first, 20 by default
rhythm logs com.example.scan                 # newest run's output
rhythm logs com.example.scan --run 42        # a specific run
rhythm --json history com.example.scan --limit 100
rhythm logs com.example.scan --host devbox   # another machine over ssh
```

`status` shows when a recorded job's last run ended, as in `exit 1, 2h ago`.

## What the recorder changes

Nothing about how the job runs. The command inherits launchd's environment,
working directory, stdin, and process group, so launchd's process-group cleanup
and keep-alive rules apply as before. Termination, interrupt, hangup, quit,
and user signals sent to the recorder are forwarded to the command. The
recorder then exits with the command's exit code, or dies by the same signal
(without a core file of its own), so launchd's own last exit stays accurate.
The command stays unreaped until the recorder is done, so a forwarded signal
can never reach an unrelated process that reused its pid.

A signal sent to the whole process group reaches the command directly and is
also forwarded, so the command sees it twice; launchd signals only the job's
main process. In a terminal's foreground, interrupt and quit come from the
terminal itself and are not forwarded. A stop that arrives before the command
starts prevents the run, which is recorded as ended by that signal.

Recording never prevents a run. If the label is invalid, the state directory
is unusable, or the ledger stays busy for two seconds, the recorder prints one
line with the error code to stderr and replaces itself with the command. A run
whose output cannot be stored is still recorded, without output. A command
that cannot be started is recorded as `spawn_failed` and exits 127 when
missing, 126 otherwise.

stdout and stderr are captured together into one output. They arrive through
separate pipes, so lines written at nearly the same moment may interleave. `--forward` also
copies them to the recorder's own stdout and stderr, so log files configured
with `StandardOutPath` and `StandardErrorPath` keep receiving output. After the
command exits, the recorder waits at most two seconds for descendants still
holding its output pipes. If any remain, such as a process that daemonized
without closing its output, a small detached drainer in its own session takes
over the pipes and keeps appending their output to the run until they close,
so those descendants never die of a closed pipe.

## Storage and retention

Runs live in the private state directory (`~/Library/Application
Support/Rhythm`, or `--data-dir ABSOLUTE_PATH`): a SQLite ledger and one
directory of output per run. Directories are `0700` and files `0600`; a
directory accessible to other users is refused. Recorders on the same machine
share the ledger safely.

Each run keeps the newest 512 KiB to 1 MiB of output. `logs` returns the newest
64 KiB by default (`--bytes`, up to 1 MiB), decoded as UTF-8 with invalid
sequences replaced; `truncated` says earlier output is not included. Each job
keeps its newest 100 runs; older finished runs and their output are removed
when a new run starts. Unfinished runs are kept for seven days, as they may
still be running. Output can contain sensitive data and never leaves the state
directory except through `logs`.

Each recorder holds a lock on its run for as long as it lives. A run without an
outcome whose lock is free, as after `SIGKILL`, is reported as `lost`. Readers
never create the state directory.

## JSON

```json
{
  "type": "history",
  "label": "com.example.scan",
  "runs": [
    {
      "id": 42,
      "label": "com.example.scan",
      "started_at": "2026-10-29T08:07:00.120Z",
      "state": "finished",
      "ended_at": "2026-10-29T08:07:03.360Z",
      "outcome": { "type": "code", "code": 1 }
    }
  ]
}
```

`state` is `running`, `finished` (with `ended_at` and `outcome`), or `lost`.
`outcome` is `code`, `signal`, or `spawn_failed`. `logs` returns
`{"type":"logs","run":{…},"output":"…","truncated":false}`; a job without
recorded runs fails with `not_found`.
