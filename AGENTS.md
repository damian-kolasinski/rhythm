# Rhythm

Project scope and technology stack have not been defined yet.

## Privacy

- Never commit or publish personal names, account identifiers, hostnames, device details, real workload labels, paths, schedules, environment metadata, or observed workload state.
- Treat machine inventory as private operational data. Use generic requirements and synthetic examples in repository documentation, tests, commits, and review artifacts.

## Architecture

- Use hexagonal architecture.
- A capability is a good candidate for a port when at least one of the following applies:
  - At least two adapters are known from the start.
  - The operator's request indicates that another adapter may be needed in the future.
  - The capability is inherently coupled to a provider strongly enough to justify a port upfront.
- Define ports from the perspective of the system and its business logic. Never adjust business logic to fit an adapter.
- Validate untrusted data at system boundaries and trust established internal invariants. Model and handle expected domain and I/O failures without adding defensive checks for hypothetical internal states.
- Keep secret values out of logs, diagnostics, and persisted artifacts, except in storage explicitly intended for secrets. Enforce authorization at system boundaries.

## Modularization

- Design for modularity: minimal interfaces, rich implementations, tightly scoped responsibilities, and minimal coupling between modules.
- Small size alone is never a reason to avoid extracting a distinct responsibility. Extract modules when they isolate a responsibility or reduce coupling.
- Each module should own one tightly scoped responsibility. For example, a transport module should contain only transport details; caching, authentication, and other responsibilities belong in separate modules.
- Catch-all modules such as `core`, `utils`, `helpers`, and `design-system` are forbidden. Organize modules by responsibility.
- Give each module a clear, tightly scoped interface. Modules may depend only on other modules' interfaces, never on their implementation details.
- Model the domain through types and data structures instead of scattered conditions. Make impossible states unrepresentable to reduce branching.

## Working

- Always prioritize usability, security, speed, and delight over code size or complexity. Code is cheap; user satisfaction is priceless.
- Before shipping work, spawn a code review subagent to review architectural and behavioral correctness, as well as security.
- When an operator's request spans multiple vertical slices, split the work into one deliverable per slice. Each slice must be independently verifiable and small enough for the operator to review comfortably.
- Verify each base slice before starting work on a slice that depends on it. Building on an unverified base makes problems harder to review and fix.
- Always verify your work with the smallest check that would fail if the intended behavior broke:
  - Pure functions: unit tests.
  - Business logic: integration tests that prove behavior without stubbing modules or units internal to the codebase.
  - Business logic involving external adapters: integration tests with fakes at the system boundary.
  - Rendering logic: golden tests covering relevant rendering states and known boundary cases.
  - UI flows: end-to-end UI tests covering crucial paths.
  - Rendering optimizations: end-to-end performance tests with a baseline and evidence of the claimed improvement, stubbing only parts that do not affect the measured behavior.

## Code review

When `mg` is installed and connected to the operator's hub, use the mergegate workflow below. Otherwise, use the normal GitHub pull request workflow.

- Publish work through the operator's mergegate hub with `mg submit`; never `git push` or `gh pr create`, even when asked to open a PR. The hub opens the GitHub PR and squash-merges it into `main` after approval.
- Follow the installed `mergegate` skill for submission, review feedback, and cleanup. If it is not loaded, read `~/.mergegate/skill/mergegate/SKILL.md` and its relevant references; do not vendor a copy into this repository.
- Run `mg init --project rhythm-recreated` once in every new clone or worktree. Fetch `mergegate` and start work on a branch from `mergegate/main`, since the clone's local `main` may be stale.
- Commit scoped changes after relevant verification, then submit with a short review guide explaining the purpose, validation, and which files to read first.
- Answer each review comment with `mg reply`, make supported changes, and submit the next revision. In Codex, leave the wake command described by the mergegate skill and end the turn instead of polling for feedback.
- Reach the hub only through `mg`; its UI and HTTP API are reserved for the operator.
- Commits and PRs use only the operator's authorship: no AI co-author trailers or identities, and no mention of models, agents, or AI tools in commit messages or PR descriptions.
