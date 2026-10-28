# Web workspace

Run the shared runtime, then open the workspace:

```sh
rhythm runtime run
# Another terminal:
rhythm ui
```

`ui` opens the loopback workspace directly. Use `ui --no-open` to print its URL
without launching a browser. `--json ui --no-open` returns `{url, demo}`. There
is no code, login form, or browser credential to copy. The runtime must already
be running. A foreground runtime and an installed runtime serve the same
embedded interface. Reloading or opening another tab works directly at that
URL, as in mergegate's local opening flow.

The workspace shows one searchable list of schedules, with failures and uncertain
outcomes first. Next-run timing stays visible on narrow screens. Search and state
filters remain in place when closing an inspected schedule. Detail links use
`#/<schedule-id>`.

## Controls and execution details

- Create paused interval, daily, or weekly schedules through the CLI using
  `schedule create --file FILE`. Trigger, overlap, missed-run, and command settings
  remain in CLI definitions and the read-only Definition tab.
- Run a schedule immediately, resume automatic triggers, or confirm pause/cancel.
  Pausing preserves active and accepted manual work. Cancellation clears active
  and queued work while leaving future triggers enabled. Pause before cancel to
  keep a schedule quiescent.
- Inspect retained history, outcomes, and output. Logs show at most the last 64 KiB
  of each retained prefix and refresh on request. Content is rendered as text.
- Investigate uncertain completion and manually clean up the named execution
  before checking the explicit acknowledgment box. Acknowledgment preserves the
  outcome and pauses the schedule; it does not kill unknown processes.

Execution state is process evidence. The UI does not infer application or
downstream health. The browser has no creation form; its trusted local API can
still create schedules. Definitions and logs stay in the current tab and private
runtime storage.

The workspace polls every two seconds while connected. It preserves keyboard
focus across refreshes, retains its last snapshot when disconnected, and disables
mutations until it can refresh. Requests time out after eight seconds. A timeout
can have an ambiguous outcome: inspect refreshed state before retrying a control.
Every control uses the same application managers, SQLite state, execution IDs,
recovery rules, and bounded logs as the CLI.

## Local trust and browser boundaries

The local opening flow follows mergegate's loopback trust model. Any process or
browser that can reach the loopback listener is a trusted local operator,
including processes belonging to other local OS accounts. Such a caller can
read definitions/logs and create or run commands with the runtime account's
permissions. This is a whole-machine trust boundary, not per-user isolation.
The client header prevents cross-site browser requests; it is public and is
not authentication against local clients. Do not proxy this listener onto a
network interface. Remote access and device admission are not implemented.

The server binds only `127.0.0.1`. Allowed Host values are exactly
`127.0.0.1:PORT` and `localhost:PORT`; browser API POSTs additionally require an
Origin of `http://` followed by that exact Host and `X-Rhythm-Client: 1`.
Missing, foreign, mismatched, or repeated Host/Origin/client headers are
rejected. No CORS headers are exposed. Cross-site web pages cannot send the
custom header without an allowed preflight, and requests without it are
refused. API bodies are bounded at 128 KiB and errors never reflect source
values. All application commands still use the same validated domain types.

The separate CLI API continues to require the private credential and no
Origin. Browser access creates no credentials, cookies, localStorage, or
sessionStorage. Direct runtime shutdown stays omitted from the browser
interface as an availability policy, not a security boundary against a trusted
local operator. Static routes are embedded, sent with `no-store`, strict CSP,
no-referrer, nosniff, and frame denial. No telemetry, external assets, service
workers, or network dependencies are needed for the interface at runtime.

## Appearance

Colors and typography use the same semantic values as mergegate's
`appearance/tokens.json`, copied into `web/src/appearance.css`. The palette
uses neutral surfaces and blue actions/selection, with separate success,
warning, and error roles. Light and dark modes follow the system preference.
Body text uses bundled DM Sans, with the same heading/body/label type scales
and tracking. Commands and output retain the system monospace stack. The font
is packaged locally under `web/DM-Sans-OFL.txt`, also embedded at
`/assets/dm-sans-license.txt`; it does not require a font CDN. Layout and
application behavior remain owned by Rhythm.

## Disposable demo

```sh
rhythm demo
# Keep browser launch manual:
rhythm demo --no-open
```

The demo uses temporary private state, synthetic schedules, and synthetic
output with the real runtime and process adapters. It visibly identifies itself
as a demo. It does not install a launchd registration or open the normal state
directory. Stop the foreground demo with Ctrl-C; the runtime stops its work and
the temporary directory is removed. Abrupt termination can leave temporary
state; it has the same private permissions as normal runtime storage.

The checked-in browser demo test exercises the same command, including an
expected failed schedule, schedule run-now/resume/pause, and logs. The recording
begins by opening the direct local URL. All example schedule names, output, and
timing are synthetic.

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

Rendering goldens cover light/dark palettes, bundled fonts, empty/loading/error,
populated, failed, recovering and uncertain states, unsupported responses,
mobile, confirmation, and literal output. Their fixtures replace only the
external HTTP boundary.
Flow tests use the real loopback server, SQLite, and process execution, including
direct CLI-URL entry, reload/new-tab access, cross-site refusal, CLI-created
interval/daily/weekly schedules, search and state filters, schedule controls,
cleanup acknowledgment, focus preservation, and connection loss/retry. Rust
tests additionally check Host/Origin/client-header enforcement, bounded
requests, generic errors, embedded assets, and demo failure cleanup.

Generate a private demo recording and screenshot with:

```sh
RHYTHM_DEMO_OUTPUT=/tmp/rhythm-web-demo pnpm --dir web demo
```

Only the synthetic demo is recorded. Traces and network captures are disabled.
Versioned mise/packslip distribution is described in [distribution.md](distribution.md).
Switching normal installed runtimes remains a separate cutover step.
