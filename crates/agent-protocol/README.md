# agent-protocol

`agent-protocol` defines the wire-facing contract shared between `agent-core`,
`agent-app-server`, clients, and external transport boundaries.

This crate should stay focused on messages and transport-safe types. It is not the place for tool
catalog policy, runtime orchestration, or storage logic.

## Responsibilities

- JSON-RPC envelope types
- request and response payload types
- exported protocol-facing enums and structs used by clients
- protocol-facing re-exports that keep the wire contract easy to consume across crates

## Non-Responsibilities

- turn execution
- built-in tool registration
- UI rendering behavior
- scheduler behavior

## Boundary With Other Crates

- `agent-core` owns stable domain types.
- `agent-protocol` adapts and re-exports the parts of that domain that belong on the wire.
- `agent-app-server` and `agent-app-server-client` use these types to preserve one stable
  transport contract above the core host.
- `agent-core` may emit and consume protocol-facing shapes at integration boundaries, but it should
  not become the transport layer itself.
