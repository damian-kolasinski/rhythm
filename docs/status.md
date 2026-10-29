# Status

`rhythm status` lists every `.plist` in `~/Library/LaunchAgents` with how it
starts and what launchd last observed. Jobs needing attention come first.

```text
  JOB                 SCHEDULE                  STATE    LAST
! com.example.scan    daily 08:07               failing  exit 1
  com.example.sync    every 5m, on queued file  running  pid 4242
  com.example.report  daily 18:30               ok       exit 0

3 jobs, 1 needs attention
```

## States

| State | Meaning | Attention |
| --- | --- | --- |
| `invalid` | The definition cannot be read, or lacks a label or program, so launchd will not run it. | yes |
| `failing` | The last run exited nonzero, by a signal, or was killed for memory, and the job is not running now. | yes |
| `down` | A keep-alive job is not running. | yes |
| `running` | launchd reports a process. | no |
| `ok` | The last run exited 0. | no |
| `idle` | Loaded but has not finished a run yet. | no |
| `unloaded` | Defined but not loaded; possibly disabled on purpose. | no |

An empty dictionary, which some installers leave to disable an agent, is not a
job and is skipped. launchd keeps only the last exit and a run count, so status
cannot say when a failure happened or what the job printed.

Each label is looked up in the graphical (`gui/UID`) domain and then the
background (`user/UID`) domain used over ssh on headless machines, up to eight
jobs at a time with a five-second limit per `launchctl` call. Status reads
definitions and `launchctl print` output only; it never loads or changes a job.
Only regular files are read, up to 1 MiB each.

## JSON

`rhythm --json status` returns one entry per host:

```json
{
  "type": "status",
  "hosts": [
    {
      "host": "local",
      "jobs": [
        {
          "label": "com.example.scan",
          "health": "failing",
          "activation": {
            "type": "triggered",
            "triggers": [{ "type": "calendar", "minute": 7, "hour": 8 }]
          },
          "observation": {
            "state": "loaded",
            "runs": 19,
            "last_exit": { "type": "code", "code": 1 }
          }
        }
      ]
    }
  ]
}
```

`activation` is `service` (`always` false for conditional keep-alive) or
`triggered` with `at_load`, `interval`, `calendar`, `path_change`,
`queue_directory`, `socket`, and `mount` triggers. An `invalid` job has no
activation. `last_exit` is `code`, `signal`, `reclaimed` (launchd stopped an
idle job that opted into idle exit, which is healthy), or `jetsam` (killed under
memory pressure), and is absent before a first exit.

## Other machines

```sh
rhythm status --host devbox
rhythm status --host local --host devbox
```

`--host` takes `local` or an ssh destination from your ssh configuration and may
be repeated. Hosts are queried concurrently with `BatchMode=yes`, a 10-second
connect timeout and a 30-second overall limit. Remote commands add mise shims,
`~/.local/bin`, and Homebrew to PATH, because noninteractive ssh shells rarely
load the profile that does.

A failing host is reported in place, as `"error"` instead of `"jobs"`, without
hiding the others, and the command exits 1. `remote_unavailable` means ssh could
not connect or timed out; `remote_incompatible` means Rhythm there is missing,
predates `status`, or answered with more than 16 MiB or an unknown format. A
host's own failure, such as `unsupported_platform`, is passed through. Health is
recomputed locally from each host's reported facts. Destinations that could be
read as ssh options are refused.
