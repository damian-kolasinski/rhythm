# Rhythm

Rhythm is an independent, agent-first tool for managing background work on one
machine. Work stays discoverable, observable, and manageable after the agent
session that created it ends. It addresses orphan processes, silent failures,
and lost context across sessions and projects.

Start with macOS and one user. Manage each machine independently; fleet
connectivity comes later. Prioritize reliability, responsiveness, and delight.

## Product surfaces

- **CLI:** the primary agent interface, with structured output, predictable
  errors, noninteractive commands, and discovery across sessions and projects.
- **Local web UI:** a human overview of running work, failures, upcoming runs,
  and anything needing attention, with inspection and basic controls.
- **Local runtime and API:** persistent execution, supervision, scheduling, and
  history shared by both interfaces, with room for a future native iOS client.
  Rhythm operates without a hosted backend or any particular agent platform.

## Tech stack

- **Rust** for the CLI and runtime; **Tokio + Axum** for asynchronous work and
  the local HTTP API.
- **SQLite** for durable state and history; files for logs and results, with
  retention policies. No separately administered database server.
- **Solid + TypeScript + Vite** for the web UI, served by Rhythm.
- **launchd** for macOS supervision, initially under the logged-in user.
- **Hexagonal architecture:** keep lifecycle behavior independent of transports,
  persistence, and platform-specific supervision.

## High-level scope

- **Jobs:** finite executions with logs, results, cancellation, cleanup, and
  progress when reported by the workload.
- **Services:** ongoing work with desired state, observed health, and explicit
  restart behavior.
- **Schedules:** persisted triggers creating job executions, with explicit
  overlap and missed-run policies.
- **Discovery and recovery:** shared inventory, execution history, and
  reconciliation after interruptions or restarts.

Begin with services, schedules, and execution records for their runs. Manage
Rhythm-owned registrations; initially only inspect externally managed work.
Report downstream completion as unknown unless there is evidence of its outcome.
Broader standalone job management follows; project-specific retries, resource
leases, and cleanup require explicit integrations.

Fleet connectivity, remote execution, native apps, and variables and secrets
design are deferred.
