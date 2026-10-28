# Owned-service contract

## Definitions and commands

A service ID starts with a lowercase ASCII letter, contains only lowercase
letters, digits, underscores, or hyphens, and has at most 63 characters.
Definitions require a command array with an absolute executable, an absolute
working directory, and an explicit restart policy: `never`, `on_failure`, or
`always`. Arguments are passed literally; a shell runs only when explicitly
specified. Unknown fields and invalid definitions fail at the API boundary.
Restart delay defaults to 1000 milliseconds and is bounded to 100–60000.

`service create --file FILE` creates a stopped service. The CLI supports
`list`, `inspect ID`, `start ID`, `stop ID`, `restart ID`, `history ID`, and
`logs EXECUTION_ID`. Accepted start/stop requests update desired state; inspect
reports the current observation. Execution IDs are monotonic within one state
directory. History returns the latest 100 records per service.

Run trusted foreground programs. The runtime supplies only a fixed system
`PATH`; it does not inherit credentials or the invoking session's environment.
Environment/secret configuration, definition editing/removal, standalone jobs,
remote control, and automatic migration remain separate capabilities. Do not
put secrets in command arguments or definition files. Captured workload output
can contain sensitive data and is intentionally kept in private log storage.

## Execution and recovery

Each execution has a durable record and a separate monitor holding an execution
lock. The monitor captures stdout/stderr and retains a process-group anchor
until cleanup is complete. Stop sends TERM, allows 500 milliseconds for cleanup,
then sends KILL to the group and reaps its directly owned processes. Cleanup also
runs after normal leader exit, so redirected background descendants do not
silently remain. Programs that detach into another session/process group are
unsupported; this is process lifecycle management, not an execution sandbox.

Desired and observed state are distinct. Observations include `stopped`,
`running`, `recovering`, `waiting`, `exited`, and `needs_attention`. Running
means an execution monitor is active; it is not an application health check.
Records distinguish exit code, signal, deliberate stop, interruption, spawn
failure, monitor loss, and unavailable output evidence.

`never` does not repeat completed executions. `on_failure` repeats nonzero exits,
signals, and spawn failures; `always` also repeats successful exits. Retry
deadlines survive runtime restart. Explicit stop disables retries. Runtime
shutdown preserves desired state, stops contained executions, and records an
interruption; the next runtime restores interrupted desired-running services
after the previous monitor releases its lock. An execution completed while
the runtime was absent still obeys its configured restart policy.

If a monitor disappears without durable completion evidence, the runtime cannot
prove its workload stopped. `needs_attention` blocks all replacement execution,
including queued restarts and after runtime restart. Investigate and manually
clean up that execution before running:

```sh
rhythm service acknowledge sample-worker --execution 1 --confirm-cleaned
rhythm service start sample-worker
```

Acknowledgment requires the latest uncertain execution ID, records its time,
preserves the original outcome, and changes desired state to stopped. The
subsequent start is explicit. The command records the operator's cleanup
confirmation; it does not itself terminate unknown processes.

## Storage and API

The default state directory is the user application support directory on macOS.
`--data-dir ABSOLUTE_PATH` selects an independent private runtime. Directories
must belong to the current user with no group/other permissions; files must be
regular, owned, and private. Symlink storage leaves are rejected. On macOS,
extended ACLs on private directories or files are rejected, including inherited
grants. SQLite uses WAL and full synchronization; execution results and runtime
access discovery are written atomically. A singleton lock excludes concurrent
runtime owners of the same directory.

Each output stream retains the first 1 MiB per execution, continuing to drain
additional output so a noisy program cannot block. `logs` returns at most the
last 64 KiB of each retained prefix as UTF-8 with invalid sequences replaced.
These are prefix captures, not rolling tails. Records and per-execution files
are currently retained indefinitely; global retention follows separately.

The runtime binds an ephemeral IPv4 loopback port. CLI requests use
`POST /v1/command` with a tagged `operation` and a bearer credential read from
private `access.json`. A new credential is generated each runtime start; it
never appears in URLs. HTTP requires the exact loopback Host and rejects Origin
headers. Browser access follows the trusted-loopback model with Host/Origin/client-header checks; see [web-ui.md](web-ui.md). JSON request bodies are
limited to 128 KiB. Transport errors and invalid input produce stable error
codes without echoing source values, paths, credentials, or OS diagnostics.
Owned-service CLI failures exit 1; successful operations exit 0.

The launchd adapter derives one registration label from the private directory
and changes only a verified owned registration. Installation is idempotent;
failed replacement attempts restore the previous registration. Missing
ownership evidence for an already-loaded target fails closed. Failure to invoke
the launcher returns an execution error, preserves an owned registration, and
never counts as evidence that the target is absent. Uninstall retains
all private execution data. `runtime stop` also shuts down a foreground API
owner and verifies that its singleton lock has been released.
