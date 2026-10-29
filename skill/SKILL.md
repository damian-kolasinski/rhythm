---
name: rhythm
description: Find, create, and debug background jobs on this machine with Rhythm, which records each launchd job's runs and output and reports failures. Use when scheduling recurring or always-on work, investigating a background job or its output, or when a session reports background jobs needing attention.
---

# Rhythm

Rhythm makes the user's launchd jobs visible and predictable. launchd keeps
running them; Rhythm records each run of the jobs that opt in and reports what
broke. Every command is noninteractive; add `--json` for structured output.

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

`rhythm adopt LABEL` records the runs of a job someone wrote by hand, changing
only its command; `unadopt` restores it. A running job needs `--restart`,
which stops the current run. Rhythm never removes or replaces a job it did not
create.

## When something failed

1. `rhythm logs NAME` shows what the failed run printed.
2. Fix the cause, then `rhythm kick NAME --wait` to confirm; it exits 1 if the
   run failed. launchd starts a job at most once every ten seconds.
3. An unrecorded job shows only its exit code: `rhythm adopt LABEL` so the next
   run keeps its output.

Tell the user about failures you find rather than silently retrying them.
Each reference below has the details: `rhythm skill show NAME`.

- `status`: states, other machines, and the JSON contract.
- `runs`: the recorder, history, output, and retention.
- `authoring`: `add`, `adopt`, `remove`, and `kick` in detail.
- `attention`: notifications and `attention --since-last-seen`.
- `agents`: the skill and session hook `rhythm skill setup` installs.
