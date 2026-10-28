# Persisted schedules

Schedules run finite, trusted foreground commands with contained cleanup,
private output capture, and durable execution records. Definitions are immutable
after creation. See [execution and storage](execution.md) for process and privacy
contracts.

## Create and inspect

Start the runtime with `rhythm runtime run`, or install it at a stable executable
location with `rhythm runtime install`. Put a definition in a private file:

```json
{
  "id": "sample-report",
  "command": ["/bin/sh", "-c", "printf 'sample report'"],
  "working_directory": "/tmp",
  "trigger": { "type": "interval", "every_ms": 60000 },
  "overlap": "skip",
  "missed": "run_once",
  "grace_ms": 1000
}
```

```sh
rhythm --json schedule create --file /tmp/sample-schedule.json
rhythm --json schedule list
rhythm --json schedule inspect sample-report
rhythm --json schedule run-now sample-report
rhythm --json schedule history sample-report
rhythm --json schedule logs 1
rhythm --json schedule resume sample-report
rhythm --json schedule pause sample-report
rhythm --json schedule cancel sample-report
```

Creation always starts paused. `run-now` is explicit and works while paused.
`resume` starts automatic triggers at the first future occurrence; repeating it
while enabled preserves the current cursor. `pause` suppresses future triggers
and drops queued automatic work, while allowing an active execution or already
accepted manual invocation to finish. `cancel` clears queued work and stops the
active process group; it leaves future triggers enabled unless paused separately.

Commands return JSON, with `--json` also making failures structured. List returns
`type: schedules`; inspect returns `type: schedule`; history
returns `type: history`. Responses are flat. Inspect exposes enabled
state, the next automatic run, one pending invocation, the latest trigger
decision, current observation, and the latest execution. History returns the
latest 100 execution records. Skipped occurrences do not create executions;
the latest decision explains `skipped_missed`, `skipped_overlap`, or `queued`.
Execution ownership includes `schedule_id` and an `invocation` identifying its
scheduled due time or manual request time. Logs use global execution IDs and
the limits and privacy rules in [execution and storage](execution.md).

## Trigger and time contracts

Intervals use `every_ms` from 100 through 31536000000 milliseconds. Their phase
is persisted and preserved across runtime restarts and clock gaps. Resuming a
paused interval starts a fresh phase one interval after resume.

Calendar triggers require an explicit IANA timezone; they do not inherit the
machine's local timezone. For example:

```json
{ "type": "daily", "hour": 9, "minute": 15, "timezone": "UTC" }
```

```json
{ "type": "weekly", "weekdays": [1, 5], "hour": 9, "minute": 15, "timezone": "Europe/Berlin" }
```

Hours are 0–23, minutes 0–59, and weekdays are distinct values 1–7 for Monday
through Sunday. A daily or weekly occurrence in a daylight-saving gap is skipped.
An ambiguous local time in a backward transition uses the earlier instant once.
Leap days and timezone offset changes are handled by the pinned timezone
library. Unknown zones and invalid or additional fields are rejected.

Scheduling compares wall-clock UTC milliseconds against persisted cursors on
each runtime tick. A backward clock change does not rewind an enabled cursor
or repeat an occurrence. Forward changes, runtime downtime, and wake-up gaps
use the missed-run policy below. These are wall-clock schedules, not monotonic
duration timers. New triggers do not occur while the runtime is stopped.

## Overlap and missed runs

Both policies are required in every definition:

| Setting | Behavior |
| --- | --- |
| `overlap: skip` | Skip an automatic occurrence while this schedule has active, recovering, or pending work. A manual run while active returns `busy`. |
| `overlap: queue_one` | Keep at most one pending invocation behind the active execution. New automatic occurrences replace an older automatic pending occurrence with the latest due time. |
| `missed: skip` | Skip the latest due occurrence when its lateness exceeds `grace_ms`. |
| `missed: run_once` | Accept the latest due occurrence even when outside the grace window; older missed occurrences are coalesced. |

`grace_ms` defaults to 1000 and accepts 0–3600000. A tick considers the latest
occurrence at or before now and moves the cursor to the next future occurrence.
If that latest occurrence is within the grace window, both missed-run policies
accept it, subject to overlap. There is no unbounded catch-up backlog. Grace is
checked when accepting a due occurrence; accepted pending work may wait longer.

An already accepted manual pending invocation is never replaced by an automatic
trigger. Another `run-now` while any invocation is already pending returns `busy`.
Queueing does not allow simultaneous executions of the same schedule. Separate
schedules may execute concurrently. Normal exits, signals, spawn failures, and
interruptions are recorded once; future triggers continue
independently. Uncertain outcomes block as described below.

## Durability, recovery, and access

The cursor and pending invocation are committed together before dispatch.
Dispatch atomically consumes that invocation and allocates an execution. If the
runtime crashes before any launch was attempted, recovery launches that same
execution ID. Once launch is attempted, completion evidence determines the
outcome. An interrupted finite run is recorded without automatically repeating
that invocation. Other pending work and future triggers remain independently
eligible after the previous execution has stopped.

Monitor loss or unavailable output evidence blocks pending and future execution,
including after runtime restart. Investigate and manually clean up the execution,
then record confirmation using its latest execution ID:

```sh
rhythm schedule acknowledge sample-report --execution 1 --confirm-cleaned
```

Acknowledgment preserves the outcome and records its time, clears pending work,
and pauses the schedule. Starting again requires an explicit `run-now` or
`resume`. The acknowledgment command does not terminate unknown processes.

The database uses schema 3 with required schedule ownership. Other schema
versions are refused without migration; use a fresh state directory.

Schedules share the authenticated CLI API and the trusted-loopback browser API
described in [web-ui.md](web-ui.md), along with private permissions, ACL checks,
and bounded definition-file reads. Both accept flat schedule commands:

```json
{ "operation": "run_now", "id": "sample-report" }
```

Secret and environment configuration are outside the current scope. Commands get
only the fixed system PATH. Keep real inventory, definitions, and logs in private
operational storage, outside review artifacts and repository history.

## Verification

Unit tests cover recurrence boundaries, interval phase, leap days, invalid
calendar input, DST gaps, and folds. Integration tests use real SQLite with
clock and process fakes only at system boundaries for policy decisions. Real
process tests verify daily/weekly dispatch at controlled clock boundaries and
both sides of the startup crash window. CLI end-to-end tests cover manual and
automatic execution, output/history, pause/resume, overlap/cancellation, runtime
crashes and downtime, uncertain monitor loss, and explicit acknowledgment.
Runtime and launchd checks also verify supervision, private storage,
authorization, bounded logs, and process containment using synthetic fixtures.
