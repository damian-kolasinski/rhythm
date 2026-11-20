---
name: rhythm
description: Find, create, and debug background jobs on this machine with Rhythm, which records each launchd job's runs and output and reports failures. Use when scheduling recurring or always-on work, investigating a background job or its output, or when a session reports background jobs needing attention.
---

# Rhythm

Rhythm shows scheduled jobs on the Mac and records runs for user launchd jobs
that opt in. macOS keeps running the jobs. Add `--json` for structured output.

## Look before you act

```sh
rhythm status                      # every user launchd job, attention first
rhythm attention                   # only what needs attention now
rhythm history NAME                # recorded runs, newest first
rhythm logs NAME                   # output of the newest run; --run ID for another
```

`NAME` is short for `dev.rhythm.NAME`; anything with a dot is a full label.
`--host DEST` reads another machine over ssh (`status` and `attention` take
it repeatedly). A job marked `!` needs attention: `failing`, `down` (a
keep-alive job not running), or `invalid` (launchd cannot load it).

Start with `rhythm status` to investigate a user LaunchAgent. In its JSON,
`recorded: true` means the job uses Rhythm's recorder. Use `rhythm ui` when
you need the wider local inventory. **All** includes Rhythm-recorded jobs
and discoverable periodic user and system launchd jobs and readable crontabs.
**Rhythm** shows **Created by Rhythm** and **Adopted by Rhythm** jobs.
**External** shows jobs Rhythm observes without recording their runs. The
window names sources it could not fully read. See the [inventory scope](status.md).

## Create jobs with `rhythm add`, never by writing a plist

```sh
rhythm add report --daily 08:07 -- /path/to/report --full
rhythm add sync --every 5m -- sync-tool --quiet
rhythm add server --keep-alive --cwd ~/project -- server
rhythm kick report --wait          # run it now and see how it ended
```

`add` gives every job the same label scheme, a PATH with mise shims and user
tools, the home directory as working directory (an agent worktree may vanish;
pass `--cwd` deliberately), and run recording. Flags: `--every 90s|5m|2h|1d`,
`--daily HH:MM`, `--weekly mon@09:00` (both repeatable), `--at-load`,
`--keep-alive`, `--env KEY=VALUE` (never secrets). `--replace` updates a job
Rhythm created; `rhythm remove NAME` deletes one.

## Existing jobs

`rhythm adopt LABEL` records an existing user LaunchAgent by wrapping its
command. `unadopt` restores the command. A running job needs `--restart`,
which stops the current run. Rhythm can replace or remove jobs it created.
System launchd jobs and cron entries are read-only in the inventory.

## When something failed

1. `rhythm logs NAME` shows what the failed run printed.
2. Fix the cause, then `rhythm kick NAME --wait` to confirm; it exits 1 if the
   run failed. launchd starts a job at most once every ten seconds.
3. For an unrecorded user LaunchAgent, launchd can show its last exit.
   `rhythm adopt LABEL` records the output of future runs.

Tell the user about failures you find rather than silently retrying them.
Each reference below has the details: `rhythm skill show NAME`.

- `status`: states, other machines, and the JSON contract.
- `runs`: the recorder, history, output, and retention.
- `authoring`: `add`, `adopt`, `remove`, and `kick` in detail.
- `attention`: notifications and `attention --since-last-seen`.
- `agents`: the skill and session hook `rhythm skill setup` installs.
