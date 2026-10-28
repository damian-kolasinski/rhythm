# Schedule execution and storage

Schedules run finite, trusted foreground commands. Every run has a durable,
monotonic execution ID, required `schedule_id`, and manual or scheduled invocation
metadata. A separate monitor holds its execution lock, captures output, and
retains a process-group anchor until cleanup finishes.

## Process lifecycle

Cancellation sends TERM, allows 500 milliseconds for cleanup, then sends KILL to
the group and reaps directly owned processes. Cleanup also runs after normal
leader exit, so redirected descendants do not silently remain. Programs that
detach into another session/process group are unsupported. This is lifecycle
management, not an execution sandbox.

Runtime shutdown records active executions as interrupted. Schedule recovery
reuses an allocated execution ID if launch was never attempted. Once attempted,
it relies on durable completion evidence. Interrupted finite invocations are
not automatically repeated; queued invocations and future triggers remain
independently eligible. [Schedule policies](schedules.md) define that behavior.

Monitor loss or unavailable output evidence blocks new runs. After inspecting
and manually cleaning up the execution, acknowledge its exact latest ID:

```sh
rhythm schedule acknowledge sample-report --execution 1 --confirm-cleaned
```

Acknowledgment preserves the outcome, clears pending work, and pauses the
schedule. It records an operator decision and does not terminate unknown
processes. Running again requires `run-now` or `resume`.

## Private storage

The default state location is the user application support directory on macOS.
`--data-dir ABSOLUTE_PATH` selects independent private state. Directories and
files must belong to the current user with no group/other access; symlink leaves
are rejected. On macOS, extended ACL grants, including inherited grants, are
rejected. A singleton lock excludes competing runtime owners.

SQLite uses WAL and full synchronization. Schema 3 contains schedules and their
executions only. Other nonzero versions are rejected with `unsupported_schema`;
there is no migration. Start in a fresh state directory when switching schema.
Execution IDs remain stable across restarts within that directory.

Each stream retains its first 1 MiB and continues draining excess output. Log
queries return at most the last 64 KiB of each retained prefix as UTF-8 with
invalid sequences replaced. These are prefix captures, not rolling tails.
Records and output are retained indefinitely; a global retention limit is not
implemented. History returns the latest 100 executions per schedule.

Commands receive only the fixed system PATH. Credentials and the invoking
session's environment are not inherited. Definitions require an absolute
executable and working directory. Arguments are literal; use an explicit shell
for shell syntax. Keep secrets out of definitions and arguments. Captured output
can contain sensitive information and stays in private log storage.

## Local API

The runtime binds an ephemeral IPv4 loopback port. CLI `POST /v1/command` uses a
bearer credential from private `access.json`, regenerated on each runtime start.
HTTP requires the exact loopback Host and no Origin. The browser endpoint uses
the [trusted-loopback browser boundary](web-ui.md).

Schedule commands are flat: `{ "operation": "run_now", "id": "sample-report" }`.
Runtime `status`/`shutdown` and execution `logs` use the same transport. There is
no service envelope or service lifecycle command. Bodies are bounded at 128 KiB;
invalid fields fail without reflecting source values or OS diagnostics.
