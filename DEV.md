# Development Notes

## Current Goal

Build `cloudagent` as a modular Rust workspace for a local/server-side agent.

This project is not a traditional monitoring platform. Server inspection, log analysis,
service checks, remote notification, and scheduled wakeups are all treated as agent
capabilities.

The product goal is:

- run a persistent agent on a server or local machine
- let the agent call tools to inspect and operate on the system
- let the agent create scheduled work for its future self
- wake the agent automatically when a scheduled task is due
- support remote conversation from phone-facing channels later
- keep the repository strongly modular, workspace-first, and suitable for long-term growth

## Architecture Direction

The repository follows a Rust workspace layout with a small number of clearly scoped crates.

Top-level layout:

```text
apps/      executable entrypoints
crates/    reusable Rust crates
web/       future web admin or frontend work
configs/   environment and app configuration
docs/      architecture and design documents
tests/     integration and workspace-level tests
data/      local runtime data for development
```

## Core Design Idea

The center of the system is the agent itself.

Important consequence:

- monitoring is not the core architecture
- scheduling is not the core architecture
- messaging is not the core architecture

Instead:

- the agent is the core
- tools are capabilities the agent can use
- scheduling is a system that can wake the agent later
- gateway modules are how the agent talks to remote users

## Crate Boundaries

### `agent-core`

Owns the core agent concepts and orchestration contracts.

Typical responsibility:

- conversation model
- messages and turns
- task and plan model
- context assembly
- tool-call abstractions
- core orchestration interfaces

It should describe how the agent thinks and advances work, but avoid taking ownership
of concrete infrastructure implementations.

### `agent-runtime`

Owns the execution lifecycle of the agent.

Typical responsibility:

- running agent conversations
- driving execution loops
- handling cancellation and timeout
- bridging scheduled wakeups into active execution

### `agent-tools`

Owns the tool system used by the agent.

Typical responsibility:

- tool definitions
- tool registry
- shell/file/http/system/service/log tools
- schedule creation and notification tools

### `agent-memory`

Owns agent-facing memory abstractions and logic.

Typical responsibility:

- conversation memory
- task memory
- wakeup context snapshots
- user and environment memory

### `agent-gateway`

Owns remote interaction entrypoints and message routing abstractions.

Typical responsibility:

- inbound/outbound remote messages
- conversation routing
- conversation mapping between remote clients and local agent execution

### `agent-scheduler`

Owns delayed and recurring execution.

Typical responsibility:

- task scheduling
- recurring plans
- wakeup triggers
- retry policy for scheduled jobs

### `storage`

Owns business persistence concerns.

Typical responsibility:

- repositories for schedules
- repositories for execution records
- repositories for memory/state artifacts

### `config`

Owns application and workspace configuration.

### `infra-*`

Own concrete infrastructure adapters.

Current split:

- `infra-shell`
- `infra-http`
- `infra-ssh`
- `infra-store`

These crates should implement low-level integrations, not business orchestration.

### `shared`

Owns cross-cutting lightweight shared types and utilities.

## Current Repository Shape

Current `crates/` layout:

```text
crates/
├─ agent-core/
├─ agent-runtime/
├─ agent-tools/
├─ agent-memory/
├─ agent-gateway/
├─ agent-scheduler/
├─ storage/
├─ config/
├─ infra-http/
├─ infra-shell/
├─ infra-ssh/
├─ infra-store/
└─ shared/
```

## Design Rules

1. Keep the root clean.
2. Prefer crate boundaries for major subsystems.
3. Do not let infrastructure details leak into `agent-core`.
4. Treat server inspection as tools, not as the whole product.
5. Keep scheduling and remote channels as first-class but separate systems.
6. Grow by adding clear interfaces before adding concrete implementations.

## Immediate Next Steps

1. Define the minimal models and traits in `agent-core`.
2. Define execution contracts between `agent-core` and `agent-runtime`.
3. Define tool registry abstractions in `agent-tools`.
4. Define memory repository abstractions between `agent-memory` and `storage`.
5. Define scheduler wakeup payloads between `agent-scheduler` and `agent-runtime`.
6. Define gateway message abstractions for future phone/web integration.
