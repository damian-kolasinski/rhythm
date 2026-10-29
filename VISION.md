# Rhythm

Agents leave background work behind: launchd agents written by hand, each with
its own label, log location, and PATH workaround. launchd runs them reliably but
remembers only the last exit code, keeps no per-run output, and tells nobody
when a job starts failing. Rhythm makes that work visible without replacing it.

## Principles

- **launchd runs jobs; Rhythm observes and records them.** No second scheduler,
  no Rhythm daemon. Existing jobs keep working unchanged until the operator
  chooses to record them.
- **One inventory.** `rhythm status` lists every user agent on this machine or
  others over ssh, with jobs needing attention first.
- **Agent-first.** Noninteractive commands, structured JSON, predictable errors,
  and fixed messages that never echo private values.
- **Failures reach someone.** A job that starts failing notifies the operator,
  and the next agent session hears what broke since it last looked.
- **One way to create jobs.** `rhythm add` and `rhythm adopt` give every job the
  same label scheme, environment, and run recording.

## Scope

User LaunchAgents on macOS, one user per machine, and other machines reached by
the operator's own ssh configuration. System daemons, fleet management,
non-launchd supervisors, and secret management are out of scope until needed.

## Architecture

Hexagonal: the job model and its health rules own the business logic. The
launchd adapter reads definitions and `launchctl` observations; the ssh adapter
reads another machine's report through the same JSON contract. Rust with no
async runtime keeps the binary small and fast to start.

Use synthetic fixtures and generic examples in repository and review artifacts.
Machine inventory, real definitions, environment details, and logs stay private.
