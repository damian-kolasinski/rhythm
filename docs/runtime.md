# Scheduler runtime

`rhythm runtime run` starts a foreground scheduler. The CLI and web UI use that
same runtime for definitions, trigger decisions, execution, and history.
`runtime status` checks its authenticated API. `runtime stop` stops it and waits
for the singleton state-directory lock to be released.

On macOS, `runtime install` registers the current executable as a Background
LaunchAgent in the user's non-graphical launchd domain. It works over SSH and
remains supervised after disconnection. Use an executable at a stable location.
An already loaded GUI registration is updated in its original domain; stop and
uninstall find owned registrations in either domain.

This supervision is user-scoped and requires an available user domain; startup
before that domain exists is not promised. Installation changes only a verified
Rhythm-owned registration. Duplicate loaded registrations, unowned collisions,
and unavailable launchers fail closed. A failed replacement restores the prior
registration and only restarts it if it was previously loaded.

`runtime stop` removes supervision and also stops a foreground API owner.
`runtime uninstall` removes the registration while preserving private state and
logs. Neither command modifies other programs' registrations.

The runtime stores schema 3 schedule data. Other schema versions require fresh
state; installing a binary does not migrate or reset an existing database.
See [execution and storage](execution.md) and [distribution](distribution.md).
