# Web workspace

Run the shared runtime, then pair a browser tab:

```sh
rhythm runtime run
# Another terminal:
rhythm ui
```

`ui` opens the loopback workspace and prints a single-use code. Paste the code
into its connection form. Use `ui --no-open` to print the URL and code without
launching a browser. `--json ui --no-open` returns `{url, code, demo}` for private
local integrations. Its output intentionally contains a temporary credential;
do not put it in logs, recordings, tickets, or review artifacts. The runtime
must already be running. A foreground runtime and an installed runtime serve
the same embedded interface.

The workspace shows Rhythm-owned services and schedules, observed execution
states, desired state, latest results, upcoming automatic runs, and failures or
uncertain completions that need inspection. It does not infer application or
downstream task health. Read-only launchd discovery stays in the CLI; discovered
registrations cannot be controlled from this workspace.

## Controls and execution details

- Search by workload name or filter by running, attention, or stopped/paused.
- Create stopped services and paused interval, daily, or weekly schedules.
  Executables and working directories must be absolute. Arguments are one
  literal argument per line; an explicit shell command is needed for shell syntax.
- Start services; confirm stop or restart. Run schedules immediately, resume
  automatic triggers, or confirm pause/cancel. Pausing preserves active and
  accepted manual work; cancellation clears active and queued work without
  disabling future triggers. Pause before cancel to keep a schedule quiescent.
- Inspect definitions, retained history, execution outcomes, and output. Logs
  show the last 64 KiB of each retained prefix and refresh on request. Content is
  rendered as text. Definitions and logs can contain sensitive operational data;
  they stay in the current tab and private runtime storage.
- Investigate uncertain completion, manually clean up the named execution, then
  explicitly check the acknowledgment box. Acknowledgment records that decision
  and preserves the original outcome. It does not kill unknown processes.

The workspace polls every two seconds while connected. It preserves keyboard
focus across refreshes, retains its last snapshot when disconnected, and disables
mutations until it can refresh. Requests time out after eight seconds. A timeout
can have an ambiguous outcome: inspect refreshed state before retrying a control.
Every control uses the same application managers, SQLite state, execution IDs,
recovery rules, and bounded logs as the CLI.

## Browser trust and credentials

Pair only a trusted browser. A paired tab is an authorized local operator and can
create and run arbitrary trusted commands with the account's permissions. It is
not a restricted viewer or a sandbox. Browser and CLI credentials are separate
to prevent accidental cross-use; they do not define different privilege levels.
Direct runtime shutdown is omitted from the browser command interface as an
availability policy, not a security boundary against an authorized operator.

Codes contain 256 random bits, expire after two minutes, and work once. Sessions
expire after eight hours, on runtime restart, or on disconnect. The runtime
keeps at most eight pending codes and sixteen sessions, evicting the oldest as
needed. Session credentials are stored only in tab-scoped `sessionStorage`
(explicit credential storage), or memory if storage is unavailable. Closing the
tab clears its storage; browser session restoration can restore tabs. Disconnect
revokes the current credential. Failed disconnect requests clear the tab but the
server credential remains valid until expiry or runtime restart.

The server binds only `127.0.0.1`, validates the exact Host and browser Origin,
and requires browser bearer authorization for commands and disconnect. CLI
requests require their private credential and no Origin. Credentials never go
into URLs, cookies, HTML, static bundles, or diagnostics. API bodies are bounded
at 128 KiB; invalid input produces fixed error codes without reflecting source
values. Static assets are embedded, fixed-route, and sent with `no-store`, a
strict CSP, no-referrer, nosniff, and frame denial. There are no external assets,
telemetry, service workers, or network dependencies at runtime. A same-account
process or privileged browser extension remains within the local trust model.

## Disposable demo

```sh
rhythm demo
# Keep browser launch manual:
rhythm demo --no-open
```

The demo uses temporary private state and synthetic services, schedules, and
output with the real runtime and process adapters. It visibly identifies itself
as a demo. It does not install a launchd registration or open the normal state
directory. Stop the foreground demo with Ctrl-C; the runtime stops its work and
the temporary directory is removed. Abrupt termination can leave temporary
state; it has the same private permissions as normal runtime storage.

The checked-in browser demo test exercises the same command, including an
expected failed service, service start/stop, schedule run-now/resume/pause, and
logs. Recording starts after pairing so credentials never appear in its pixels.
All example workload names, output, and timing are synthetic.

## Build and verification

The frontend uses pinned Solid, TypeScript, Vite, and Zod packages. Rebuild assets
before compiling Rust after a frontend edit:

```sh
pnpm --dir web install --frozen-lockfile
pnpm --dir web build
cargo build --locked
pnpm --dir web exec playwright install chromium
pnpm --dir web test
```

The generated `web/dist` assets are committed so a normal Rust build works
without Node and produces a self-contained binary. `pnpm --dir web exec prettier
--check src tests '*.ts' '*.json' index.html` checks source formatting. Browser
tests use a pinned Chromium, UTC, synthetic fixtures, isolated temporary state,
and one worker. Run them after Cargo finishes rebuilding the binary; do not
relink it while a real-process test is active.

Rendering goldens cover connection, empty/loading/error, populated, failed,
recovering and uncertain states, unsupported responses, mobile, confirmation,
and literal output. Their fixtures replace only the external HTTP boundary.
Flow tests use the real loopback server, SQLite, and process execution, including
single-use pairing, reload/disconnect/expiry, service creation and controls,
interval/daily/weekly creation, schedule controls, cleanup acknowledgment,
search, focus preservation, and connection loss/retry. Rust tests additionally
check credential lifetime, origin/Host enforcement, bounded requests, generic
errors, embedded assets, and demo failure cleanup.

Generate a private demo recording and screenshot with:

```sh
RHYTHM_DEMO_OUTPUT=/tmp/rhythm-web-demo pnpm --dir web demo
```

Only the synthetic demo is recorded. Traces and network captures are disabled.
Versioned mise/packslip distribution and switching installed runtimes follow in
the next slice; this change does not cut over existing workloads.
