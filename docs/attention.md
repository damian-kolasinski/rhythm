# Attention and notifications

Background work fails quietly. Rhythm tells the operator when a recorded job
starts failing, and tells the next agent session what broke since it last
looked.

## Notifications

When a recorded run fails after a successful one, or after none, the recorder
posts a desktop notification such as `dev.rhythm.report failed: exit 1. See
rhythm logs report`; a command that cannot start at all counts. When a failing
job succeeds again, it posts that it recovered. Repeated failures stay quiet,
so a job failing every five minutes notifies once. A run ended because the job
was asked to stop, as when it is unloaded or the user logs out, is recorded as
`stopped` and is neither a failure nor a recovery; nor are lost runs.

Delivery never delays the job. It is configured in the job's own environment,
for example with `rhythm add --env`:

| Variable | Effect |
| --- | --- |
| none | macOS notification through `osascript` |
| `RHYTHM_NOTIFY=off` | no notifications for this job |
| `RHYTHM_NOTIFY_COMMAND=/absolute/path` | runs that program with the title and message as its two arguments instead, such as a script that pushes to a phone from a headless machine |

Desktop notifications need a graphical login; jobs in the background domain
of a headless machine need a notify command. Delivery runs detached, so a
notifier that cannot reach a display never affects the job.

## Attention

```sh
rhythm attention                                   # everything needing attention now
rhythm attention --since-last-seen --as claude-code
rhythm --json attention --since-last-seen --as codex --host local --host devbox
```

Without `--since-last-seen`, `attention` lists the jobs `status` marks as needing
attention. With it, each consumer named by `--as` (lowercase letters, digits,
and hyphens; `default` when omitted) gets only what changed since its previous
look:

- a job that newly needs attention, or failed again since, which launchd's run
  count or a newer recorded run shows;
- a recorded job that failed and recovered in between, which would otherwise
  go unseen.

Each consumer's last look is kept privately in the state directory, so two
agent tools never take each other's news. A run still in progress is not part
of a look until it ends, and a recovery is reported only once the latest run
succeeded. Locally, a look is saved only after its output was written, so an
interrupted hook loses nothing. Another machine saves its own look before
answering over ssh, and keeps one place per consumer name: give each machine
that asks it a distinct name, such as `--as laptop-claude-code`. Two sessions of
the same consumer at once may both report the same news; neither misses any.
With nothing to report, text output is empty, which keeps session hooks
silent; a person in a terminal sees `Nothing needs attention.`

```text
Background jobs need attention:
! dev.rhythm.report failed: exit 1, 2h ago. See `rhythm logs report`.
~ dev.rhythm.sync recovered after 2 failed runs since last check (runs 41–43). See `rhythm history sync`.
! devbox: com.example.watch failed: signal 9. Record its runs by running `rhythm adopt com.example.watch` there.
```

`attention` exits 0 whatever it reports, and 1 only when a host could not be
checked. JSON has the same per-host shape as `status`:
`{"type":"attention","hosts":[{"host":"local","items":[…]}]}`, where each item
has `label`, `health`, and, when known, `last_exit`, `last_run`, and
`recovered_from` (`count` of failed or lost runs with the `first` and `last`).
Hints for another machine include its `--host`.

## Session hooks

Claude Code adds a `SessionStart` hook's output to the session's context. In
`~/.claude/settings.json`:

```json
{
  "hooks": {
    "SessionStart": [
      {
        "hooks": [
          {
            "type": "command",
            "command": "rhythm attention --since-last-seen --as claude-code"
          }
        ]
      }
    ]
  }
}
```

For tools without session hooks, an instruction in the user-level agent
instructions does the same: "At the start of each session, run `rhythm attention
--since-last-seen --as codex` and mention anything it reports."
