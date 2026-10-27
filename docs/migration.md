# Implementation and migration plan

## Privacy

Machine inventory is private operational data. Keep names, account identifiers,
hostnames, device details, real workload labels, paths, schedules, environment
metadata, and observed workload state out of commits, review descriptions, and
published artifacts. Repository documentation uses generic requirements and
synthetic examples only. Do not persist raw discovery output in the repository.

## Migration requirements

- Preserve arguments, working directory, required environment configuration,
  restart conditions and throttles, process context, and output routing.
  The discovery DTO intentionally cannot serve as an import file.
- Support interval, daily, and weekly triggers, startup behavior, wildcard
  calendar components, explicit overlap policies, and missed-run policies.
  Verify local-time behavior across DST, sleep/wake, and clock changes.
- Track launcher results separately from downstream completion. A launcher can
  skip unnecessary work or start a detached task; report downstream status as
  unknown until an explicit integration provides completion evidence.
- Preserve external ownership. Workload installers can recreate registrations;
  adoption requires an owner-supported opt-out or an explicit integration.
- Stage one workload at a time. Retain rollback data in private operational
  storage, stop the old trigger before enabling the new one, and verify that
  exactly one runner owns the workload.
- Design environment and secret handling before adopting workloads that depend
  on them. Never store secrets in generic configuration, logs, or review files.
- Start with disposable fixtures and a low-risk finite schedule. Verify existing
  workload failures independently from proving equivalent execution.

## Deliverable sequence

Each row is independently verifiable. Verify and commit a base before starting
a dependent row. Submit reviewable branches through mergegate; a release and
live workload cutover follow reviewed implementation. Never include personal or machine-specific inventory in repository documentation or review artifacts.

| Slice | CLI/runtime | UI | Verification |
| --- | --- | --- | --- |
| 1. Read-only discovery (this branch) | `discover [--json]`, typed inventory port, launchd adapter, safe diagnostics, migration requirements | Define inventory contract for later reuse | Parser tests, real-file integration with fake external launchctl, real CLI E2E, private validation on target machines |
| 2. Owned services and execution records | Validated service definitions; SQLite persistence port; process execution and platform supervision ports; foreground runtime; install/status/stop runtime; create/list/inspect/start/stop/restart services; execution IDs, desired/observed states and exit reasons; bounded private logs | Shared authenticated local API; service list/detail, failures, logs, start/stop/restart | Durable-state/process integration; process-group cancellation; crash/restart reconciliation; no duplicate supervisors; real launchd installation with disposable service |
| 3. Persisted schedules | Interval/daily/weekly definitions, run-now/pause/resume; explicit overlap and missed-run policy; durable executions and next-run calculation | Upcoming runs, schedule details/history, pause/resume/run-now | Clock-boundary tests; fake clock at system boundary; real finite processes; sleep/restart and overlap integration |
| 4. Human overview and migration rehearsal | Shared CLI/application contracts; structured API errors; origin/auth boundaries; safe output and retention; validate/stage migration with rollback | Solid/TypeScript/Vite served by Rust; overview, search, attention states, ownership, confirmation for consequential controls | Rendering goldens for empty/loading/error/running/failed/unknown; browser E2E for controls/history/logs and authorization; fixtures before adoption |
| 5. Distribution | Pinned mise development tasks; versioned archive with embedded UI; packslip attestations and mise installation instructions | Served assets come from installed binary | CI builds/tests; archive extraction E2E; signed release installation in isolated mise data directory |
| 6. Distributed cutover | Pin first released version with mise on each host; move runtime registration to installed executable; restart/reconcile against same state; retain rollback | Verify UI against installed runtime | Binary provenance/version and launchd executable path; rerun service/schedule/UI E2E; runtime restart preserves history/logs; only then migrate selected live work |

### CLI behavior needed beyond discovery

Use noninteractive commands with stable JSON schemas, documented exit codes,
actionable errors, stable workload/execution IDs and idempotent desired-state
operations. Discover externally managed work across projects while keeping
mutation confined to Rhythm-owned registrations. Inspect should explain last
failure, next run, ownership and migration blockers. Logs need bounded reads,
retention, private file permissions and explicit treatment as sensitive content.

### Local API and UI boundaries

The CLI and UI must call the same application services. Keep Axum transport,
authentication/origin checks, SQLite storage, process execution, scheduling,
retention, asset serving, and platform supervision in separate modules with
narrow interfaces. Bind HTTP to loopback and enforce authorization at its
boundary; loopback alone is insufficient. Avoid embedding capabilities in
URLs, logs, or public artifacts. UI display of external labels, output, and
errors must not interpret HTML. Start with an overview grouped by owned versus
external work, an attention list, upcoming triggers, and workload details with
execution history; expose controls only for owned work.

## Distribution

Use mise and packslip for versioned distribution. Build target-specific archives
with the CLI at archive root and the UI embedded. Sign release artifacts with
packslip in an OIDC-enabled job, then publish archives and the signature bundle
together. Pin the installed release version and verify provenance before cutover.
Do not fall back to a local build when release verification fails.

The release matrix must match supported platforms and corresponding verification.
Validate the published artifact in an isolated installation, then verify runtime
registration points to the installed executable. Repeat service, schedule, and
UI checks after switching, retaining private rollback data until they pass.

## Discovery verification

Automated coverage includes XML and binary registrations, interval and calendar
boundaries, implicit startup behavior, bootstrap-context uncertainty, partial
inventories, special files, secret omission, terminal rendering goldens, and a
CLI end-to-end check using real launchctl with isolated synthetic fixtures.
These checks validate discovery; runtime, UI, distribution, and cutover require
their own later verification.
