# Rhythm

Rhythm is a small, agent-first scheduler for recurring commands on one machine.
A schedule remains discoverable across sessions, explains its next run and last
outcome, and provides retained execution history and output.

## Product surfaces

- **CLI:** create and inspect schedules, enable/pause triggers, run immediately,
  cancel active/queued work, and investigate uncertain execution. Commands are
  noninteractive with structured output and predictable errors.
- **Local web UI:** one searchable schedule list, attention states, upcoming
  timing, execution history, logs, and basic controls. Configuration stays in
  CLI definitions.
- **Local runtime:** durable interval/daily/weekly triggers, explicit overlap and
  missed-run policies, contained execution, and recovery shared by both clients.

Scope is finite scheduled execution on one machine. General process supervision,
external workload discovery, standalone job management, fleet control, native
clients, and secret/environment configuration are outside the product scope.
The runtime itself uses launchd supervision on macOS.

## Architecture

Use hexagonal architecture: schedule policy owns its business rules; process,
clock, storage, platform supervision, and transport adapters serve those rules.
Keep execution ownership schedule-specific and validate untrusted input at the
CLI, HTTP, and storage boundaries.

Rust, Tokio, and Axum provide the CLI/runtime, SQLite stores definitions and
execution records, and Solid/TypeScript/Vite provide the embedded UI. Private
files retain bounded output. Execution state describes process evidence;
application or downstream health requires evidence from the command itself.

Use synthetic fixtures and generic examples in repository and review artifacts.
Machine inventory, real definitions, environment details, and logs stay private.
