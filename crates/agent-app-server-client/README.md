# agent-app-server-client

`agent-app-server-client` is the shared client facade for talking to
`agent-app-server`.

It exists so CLI-style product surfaces can use one client model for:

- in-process app-server connections
- stdio app-server connections
- shared event handling behavior
- shared shutdown behavior

This crate is not agent core logic and it is not an app server. It is the
consumer-side access layer for the app-server protocol.

## Purpose

Different product surfaces should not each invent their own app-server client.
That leads to drift in:

- command dispatch
- event buffering and lag handling
- shutdown flow
- in-process versus transport-backed startup behavior

This crate centralizes those behaviors behind one client API.

## What This Crate Owns

- the unified `AppServerClient` facade
- in-process client startup and lifecycle
- stdio client startup and lifecycle
- event forwarding and lag signaling
- thin convenience helpers for common app-server commands

## What This Crate Does Not Own

The following do not belong here:

- the core agent execution model
- session and turn business logic
- tool execution
- conversation history semantics
- server-side request coordination

Those live in `agent-core` and `agent-app-server`.

## Relationship To `agent-app-server`

This crate is the client-side counterpart to `agent-app-server`.

It should preserve app-server semantics while hiding connection details from
product surfaces. In-process mode should still behave like app-server, not like
a private direct call path that invents a second contract.

That means:

- commands still go through app-server command handling
- events still arrive as app-server messages
- lag and disconnect behavior remain explicit

## Relationship To Product Surfaces

CLI and future UI clients should prefer this crate when they want to speak to
the app-server layer.

They should not duplicate:

- in-process request/event channels
- stdio protocol handling
- event delivery and buffering logic

## Design Principles

- Keep client semantics aligned with app-server semantics.
- Keep in-process and stdio behavior close enough that products do not need
  separate mental models.
- Keep the facade transport-aware but business-logic-light.
- Do not bypass app-server just because the host is in-process.

## Source Layout

- `src/in_process`:
  in-process client facade and event worker
- `src/stdio`:
  stdio-backed client transport
- `src/lib.rs`:
  top-level client facade and shared event helpers

## Boundary Rule

If a change is about how a client connects to, sends commands to, or receives
events from the app-server, it belongs here.

If a change is about how the server handles a command or how the agent itself
executes a turn, it belongs elsewhere.
