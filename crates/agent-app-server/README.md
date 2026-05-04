# agent-app-server

`agent-app-server` exposes `cloudagent` as a session-oriented application service.

This crate sits between product clients and `agent-core::AgentHost`.
It does not own the agent's reasoning model, turn loop, tool contracts, or
history model. Those belong to `agent-core`.

Its job is to take a live `AgentHost` and present it through an application
protocol that clients can drive through:

- in-process channels
- stdio transport
- future app-facing transports

## Purpose

This crate exists so product clients do not need to call low-level host methods
directly or reimplement session orchestration concerns such as:

- command routing
- turn submission and interruption
- session listing, creation, reset, and archive flows
- server-request delivery and resolution
- notification projection for clients

In short:

- `agent-core` answers: how does the agent run?
- `agent-app-server` answers: how does a client talk to the running agent?

## What This Crate Owns

- app-facing command routing
- in-process app-server startup
- stdio app-server loop
- session service orchestration
- turn service orchestration
- server-request coordination
- notification projection into app-server message shapes

These are application-service responsibilities, not core agent responsibilities.

## What This Crate Does Not Own

The following do not belong here:

- the turn execution backbone
- concrete tool implementations
- tool routing semantics
- model request/response domain language
- core conversation history contracts
- rollout reconstruction rules

Those all belong below this layer, primarily in `agent-core` and `agent-tools`.

## Relationship To `agent-core`

`agent-app-server` depends on a ready-to-use `agent_core::AgentHost`.

It should treat `AgentHost` as the core execution entry point and avoid growing a
parallel runtime model. This crate may sequence host calls, translate them into
app-server notifications, and manage client interaction state, but it should not
redefine the meaning of:

- turns
- tool calls
- approvals
- conversation history
- rollout facts

## Relationship To `agent-app-server-client`

`agent-app-server` is the server side.

`agent-app-server-client` is the client facade used by UI surfaces and other
product adapters. The client crate should speak to this crate rather than
duplicating request/event wiring itself.

## Design Principles

- Keep `AgentHost` as the only core execution authority.
- Keep app-server behavior protocol-oriented, not core-domain-oriented.
- Keep request/notification transport concerns here, not in `agent-core`.
- Prefer one application protocol path for all clients over special-case UI
  integrations.
- Do not let this crate regrow a second runtime layer.

## Source Layout

- `src/app`:
  in-process server startup and application-level bootstrap
- `src/projection`:
  projection from core events into app-server notifications
- `src/routing`:
  command dispatch and request routing
- `src/server_request`:
  server-request coordination and reply lifecycle
- `src/session`:
  session-oriented operations over conversations
- `src/turn`:
  turn-oriented operations over a live `AgentHost`
- `src/transport`:
  stdio transport loop and framing

## Boundary Rule

If a change alters how the agent itself reasons, executes a turn, runs tools, or
stores durable conversation facts, it probably belongs in `agent-core`.

If a change alters how a client drives that agent through commands, notifications,
or request/response coordination, it likely belongs here.
