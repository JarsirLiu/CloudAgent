# agent-protocol

`agent-protocol` defines the wire-facing contract shared between runtime, clients, and external
service boundaries.

This crate should stay focused on messages and transport-safe types. It is not the place for tool
catalog policy, runtime orchestration, or storage logic.

## Responsibilities

- JSON-RPC envelope types
- request and response payload types
- exported protocol-facing enums and structs used by clients
- compatibility re-exports that make the agent surface easier to consume across crates

## Non-Responsibilities

- turn execution
- built-in tool registration
- UI rendering behavior
- scheduler behavior

## Boundary With Other Crates

- `agent-core` owns stable domain types.
- `agent-protocol` adapts and re-exports the parts of that domain that belong on the wire.
- `agent-runtime` emits and consumes these protocol types during live execution.
