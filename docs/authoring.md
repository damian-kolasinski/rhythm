# Creating and adopting jobs

Every agent used to write launchd definitions its own way: a new label scheme,
a new log location, a new PATH workaround. `rhythm add` is one predictable way
to create a job, and `rhythm adopt` brings an existing job under recording
without changing anything else about it.

## Add

```sh
rhythm add report --daily 08:07 -- /path/to/report --full
rhythm add sync --every 5m --at-load -- sync-tool --quiet
rhythm add review --weekly sat@09:00 --weekly wed@18:30 -- ./weekly-review
rhythm add server --keep-alive --cwd ~/project --env MODE=production -- server
rhythm add cleanup -- /path/to/cleanup          # only runs when kicked
rhythm add report --replace --daily 06:00 -- /path/to/report
```

`add NAME` creates `~/Library/LaunchAgents/dev.rhythm.NAME.plist` and loads it.
Names are lowercase letters, digits, and hyphens. Everywhere a job is named,
`NAME` is short for `dev.rhythm.NAME`; any argument with a dot is a full label.

| Flag | Starts the job |
| --- | --- |
| `--every 90s\|5m\|2h\|1d` | every interval (`StartInterval`) |
| `--daily HH:MM` | daily at a local time; repeatable |
| `--weekly DAY@HH:MM` | weekly, as in `mon@09:00` or `saturday@9:00`; repeatable |
| `--at-load` | when loaded, such as at login, in addition to other triggers |
| `--keep-alive` | at once, and again whenever it exits; not combined with others |
| none | only when started with `rhythm kick` |

Times are in the machine's local time zone, as launchd interprets them.

The definition is the same for every job:

- The command runs through `rhythm run`, so every run is recorded. Jobs reach
  Rhythm through `bin/rhythm` in the state directory, a link to the executable
  that last created or adopted a job. Installs are versioned and an upgrade can
  remove the old executable; `status` or `kick` from the new one repairs a
  dangling link, and `adopt` repoints a job recorded through another path.
- A bare command name is resolved on the caller's PATH; relative paths are made
  absolute against the current directory. Symlinks such as mise shims are kept
  as named, since their name selects the tool. The command must be a file this
  user may execute.
- The working directory is the home directory unless `--cwd` names another.
  Agent worktrees come and go; a job outlives them.
- PATH puts `~/.local/bin`, mise shims, and Homebrew before the system
  directories, because launchd's own PATH has none of them. `--env KEY=VALUE`
  adds or overrides variables. Definitions are plain files: never put secrets in
  them.
- The recorder's own diagnostics go to `recorder.log` in the state directory.
- With a graphical login the job loads into the `gui` domain; on a headless
  machine it loads into the background `user` domain with
  `LimitLoadToSessionType = Background`.

`add` refuses a label that is defined or loaded, and never overwrites a file
already at `dev.rhythm.NAME.plist`, whatever it contains. `--replace` updates a
job Rhythm created under that name, reloading it and keeping its history; it
never replaces a job someone else wrote, and creates the job if none exists.
If launchd refuses a definition (`refused`), the previous one is restored and
reloaded; if even that fails, the error is `rollback_incomplete`.

## Adopt and unadopt

```sh
rhythm adopt com.example.scan
rhythm unadopt com.example.scan
```

`adopt` changes only the command: it starts through `rhythm run` with the same
arguments. Every other key, such as triggers, environment, priority, and log
paths, is kept, as are the file's permissions. A loaded job's definition is the
file launchd reports it was loaded from; one outside LaunchAgents is not
changed. A job with its own `StandardOutPath` or `StandardErrorPath`
records with `--forward`, so those files keep receiving output. A loaded job is
reloaded so the change takes effect; reloading stops a running instance, so a
running job needs `--restart`, and a job that runs at load starts again right
away. Adopting a recorded job again does nothing, unless it records through
another Rhythm executable or state directory, which it is repointed to.

`unadopt` restores the original command and keeps the recorded history.

Rhythm refuses to rewrite a definition that is a symlink, or one with both
`Program` and `ProgramArguments` (whose first argument is then only a display
name), because it cannot change those safely.

## Remove

`rhythm remove NAME` unloads and deletes a job created by `add`. Recorded
history stays until it is pruned. Jobs written by other tools are never removed;
`unadopt` them instead.

## Kick

```sh
rhythm kick report               # start now
rhythm kick report --wait        # start, wait, print output and outcome
rhythm --json kick report --wait --timeout 120
```

`kick` starts a loaded job immediately; a job that is already running is
refused with `busy`. launchd starts the same job at most once every ten seconds,
so a kick soon after another run waits for that interval. `--wait` waits for
launchd to report the run finished, up to `--timeout` seconds (600 by default),
then prints the newest 4 KiB of the output of the run this kick recorded, if
any, and the outcome. It
exits 1 when the run failed. If the wait times out, the run continues.

```json
{
  "type": "kicked",
  "label": "dev.rhythm.report",
  "finished": {
    "exit": { "type": "code", "code": 0 },
    "run": { "id": 42, "state": "finished", "outcome": { "type": "code", "code": 0 }, "…": "…" },
    "output": "…",
    "truncated": false
  }
}
```

`add`, `adopt`, `unadopt`, and `remove` return `{"type":"added"|"adopted"|
"unadopted"|"removed","label":…}`; `adopted` includes `changed`, and `added`
includes `replaced`.
