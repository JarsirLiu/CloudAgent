# agent-memory

`agent-memory` is the concrete long-term memory service for `cloudagent`.

This crate sits below `agent-core` and implements the memory behavior that the
core host can call through the `MemoryBackend` contract. It is not part of the
core turn language, and it is not a wire or UI layer.

## Purpose

`agent-core` needs a stable way to:

- inject memory context into a turn
- decide whether a finished conversation should be persisted
- persist reusable facts after a turn completes

Those are core needs, but the concrete policy for how memory is stored,
summarized, and laid out on disk should remain replaceable. `agent-memory`
exists to provide that concrete implementation without pushing storage-oriented
or policy-heavy logic back into `agent-core`.

## What This Crate Owns

- the concrete `LongTermMemoryFacade`
- memory load planning
- persistence triggers for deciding when to write memory
- memory summary extraction from conversation history
- concrete memory service logic backed by `infra-store`

## What This Crate Does Not Own

The following do not belong here:

- turn orchestration
- conversation history truth
- tool execution
- wire protocol types
- product-facing UI or transport behavior
- the abstract memory contract itself

The contract belongs in `agent-core`. This crate implements that contract.

## Relationship To `agent-core`

`agent-core` owns the `MemoryBackend` trait and decides when memory should be
consulted during the agent lifecycle.

`agent-memory` provides one concrete backend that satisfies that contract. This
lets the core host stay stable while memory behavior remains independently
evolvable.

This boundary is intentional:

- `agent-core` owns the execution skeleton
- `agent-memory` owns one concrete memory strategy

That keeps the core understandable without coupling it to a single persistence
layout or memory policy.

## Relationship To `infra-store`

`agent-memory` depends on `infra-store` for concrete file-backed persistence.
It should not reimplement low-level file layout primitives when those can live
in a reusable infrastructure crate.

## Design Principles

- Keep the abstract memory contract in `agent-core`.
- Keep concrete memory policy outside the core.
- Treat memory persistence as a backend concern, not a second history system.
- Prefer deterministic, inspectable persistence behavior over opaque heuristics.
- Keep the concrete implementation replaceable without changing turn semantics.

## Source Layout

- `src/facade.rs`:
  concrete `MemoryBackend` implementation used by `AgentHost`
- `src/service.rs`:
  concrete persistence operations over the backing repository
- `src/load_plan.rs`:
  memory injection planning for turn-time context loading
- `src/trigger.rs`:
  persistence decision logic
- `src/model.rs`:
  crate-local memory configuration and plan types

## Boundary Rule

If a change affects how the core host talks about memory, it probably belongs
in `agent-core`.

If a change affects how memory is loaded, summarized, or persisted in this
particular implementation, it belongs here.
