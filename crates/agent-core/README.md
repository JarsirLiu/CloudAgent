# agent-core

`agent-core` is the business-logic nucleus of `cloudagent`.

This crate defines the stable agent domain model and the core execution language that every
product surface must share. The CLI, app server, future web clients, and any other frontends
should all be able to talk about a turn, a tool call, a tool result, a history item, and a
rollout item in exactly the same way. That common backbone lives here.

The long-term direction is to keep the agent's reasoning model, turn model, context model, and
tool-result model concentrated in one place so the system is understandable without tracing a task
through many unrelated crates. `agent-core` is therefore not a misc types crate. It is the place
where the core agent language is defined.

## What This Crate Is For

This crate exists to keep the agent coherent across different products and runtimes.

When a user submits a request, the system should have one shared understanding of:

- what a conversation is
- what a turn is
- what events can happen during a turn
- what facts can be preserved in history
- how tool calls and tool results are represented
- how context is prepared before a model request
- how rollout data is projected back into conversation and transcript views

If those concepts drift between CLI code, web code, runtime code, and tool code, the architecture
becomes hard to reason about. `agent-core` prevents that drift by owning the stable domain model.

## Core Design Standard

The design standard for this crate is:

- one shared domain model for the agent
- one stable representation of turns, messages, tools, and results
- one context-shaping model for history, budgeting, filtering, and compaction
- one projection model from internal facts to conversation and transcript views

In other words, this crate should define the concepts that remain true no matter which product
surface is driving the agent or which tool source is being called.

## What Belongs Here

The following belong in `agent-core`:

- conversation and response item structures
- turn lifecycle structures and event types
- tool contracts such as specs, identities, calls, results, and execution context
- the concrete core host object that drives turns through injected backends
- backend traits for conversation storage, rollout recording, memory, model access, and tool execution
- tool-batch orchestration and approval flow that are part of the agent execution skeleton
- context preparation, filtering, budgeting, and compaction logic
- model request and response domain types
- rollout item structures
- projections that rebuild conversation and transcript views from recorded facts

These are all part of the agent's core business language. They should be stable, reusable, and
independent from any one frontend or transport.

## What Does Not Belong Here

The following do not belong in `agent-core`:

- terminal UI rendering
- browser or web UI rendering
- app-server transport concerns
- concrete storage backends
- concrete built-in tool implementations
- shell process management details
- MCP transport protocol details

Those concerns can depend on `agent-core`, but they should not redefine its concepts.

## Core Architecture

The core architecture is organized around a small number of durable domains.

### Conversation

The conversation domain defines the durable message history used by the agent:

- user messages
- assistant messages
- tool outputs
- conversation turns
- transcript-facing items derived from recorded facts

This domain is responsible for preserving what happened, not for deciding how a specific client
renders it.

### Turn

The turn domain defines the runtime language of a single agent turn:

- turn lifecycle phases
- item started / delta / completed events
- turn state
- server request and approval request structures

This is the shared event model that different runtimes and products must agree on.

### Tool

The tool domain defines the stable contract between the core agent and the tool system:

- `ToolSpec`
- `ToolIdentity`
- `ToolCall`
- `ToolResult`
- structured tool-result payloads
- tool execution context

This layer does not own concrete built-in tools. It owns the language used to describe and record
tool use.

It also owns the agent-side execution skeleton around tool use:

- tool-batch orchestration
- approval request flow
- cancellation semantics during tool execution
- rollout and history recording of tool results

Concrete tool implementations still live outside this crate.

### Context

The context domain prepares model input from history and system state:

- context budgeting
- input filtering
- history compaction
- environment context
- model-request assembly helpers

The key rule here is that model input shaping must be deterministic and inspectable. It should not
be hidden inside UI code or scattered across adapters.

### Projection

The projection domain converts core recorded facts into higher-level views:

- transcript items
- conversation history rebuilds
- rollout-to-turn reconstruction
- tool event summaries

Projection exists so clients and runtimes can consume the same facts without each inventing their
own interpretation layer.

### Rollout

The rollout domain defines what gets persisted for later reconstruction of a session or turn. It is
the stable recorded form of agent progress and should be sufficient to rebuild user-facing history
without depending on transient UI state.

### Model

The model domain defines the backend-agnostic request/response language used to talk to the LLM:

- request messages
- tool specs passed to the model
- model responses
- token usage

Model adapters outside this crate should translate external APIs into this shared model, not create
parallel response shapes.

### Host

The host domain is the concrete entry point used by product adapters to run the agent through
`agent-core`.

It owns:

- `AgentHost`, the concrete host object that exposes the core agent API
- backend-facing contracts for store, rollout, and memory services
- the stable construction shape used to inject concrete model, tool, memory, and storage backends

This keeps the execution backbone in one place while still allowing concrete adapters to stay
outside the core.

## Why This Crate Matters For Multi-Product Support

`cloudagent` is expected to support more than one frontend and more than one deployment style.

That makes a strong core more important, not less important.

The CLI, web application, app server, and future integrations should be thin product surfaces on
top of one agent language. If each product ends up owning its own turn logic, history logic, or
tool-result logic, the system will fragment and become difficult to evolve safely.

`agent-core` is the place where we prevent that fragmentation.

## Relationship To Other Crates

### `agent-tools`

`agent-tools` owns the concrete tool system:

- built-in tool implementations
- tool descriptors
- tool routing
- tool-level approval and execution metadata
- MCP-backed tool integration

`agent-core` does not implement tools. It defines the contracts that tools must satisfy.

### Host Construction

Product adapters should depend on a stable host object and construction shape from `agent-core`,
not on a parallel orchestration layer outside core.

The intended shape is:

- `agent-core` owns the execution skeleton and the host contracts
- product adapters assemble concrete dependencies such as model backend, tool backend, memory
  backend, and storage backend
- those assembled services are handed into `agent-core`, typically through `AgentHost::new(...)`
- `agent-core` drives turns, approvals, history reconstruction, and rollout recording through
  those injected contracts

This keeps the execution backbone concentrated in one crate while still keeping concrete adapters
outside the core.

### Product Adapters

CLI, app-server, and future web adapters should stay thin. They are responsible for:

- loading configuration
- constructing concrete dependency implementations
- wiring approvals and transport callbacks
- calling the host-construction entry exposed by `agent-core`

They should not own a parallel runtime layer with its own interpretation of turns, history, or
tool execution.

### Product Surfaces

Crates such as CLI, app server, and future web adapters should stay outside the core. They should
consume `agent-core` concepts and avoid redefining conversation, turn, or tool semantics locally.

## Design Principles

- Keep the core domain language explicit and strongly typed.
- Prefer one stable concept over many near-duplicates in different crates.
- Keep projections derived from core facts instead of inventing frontend-local truth.
- Treat structured tool results as durable facts, not display-only payloads.
- Keep model-input shaping visible and inspectable.
- Make cross-product consistency more important than local convenience.
- Keep the agent execution backbone in `agent-core`, even when concrete services are injected from
  outside it.

When in doubt, prefer the design that makes it easier to answer:

- what happened in this turn?
- what facts are durable?
- what did the model actually see?
- what tool was actually called?
- can another frontend reconstruct the same truth from recorded data?

## Source Layout

- `src/context`:
  context preparation, filtering, budgeting, compaction, and environment shaping
- `src/conversation`:
  durable conversation history structures and helpers
- `src/host`:
  `AgentHost`, host construction parts, and backend-facing contracts for store, memory, and
  rollout services used by the core
- `src/model`:
  backend-agnostic model request and response contracts
- `src/observability`:
  audit, token-budget logging, and core execution diagnostics
- `src/projection`:
  transcript and conversation reconstruction from core facts
- `src/rollout`:
  persisted turn and history item representations
- `src/state`:
  in-memory agent conversation state and active-turn coordination
- `src/tool`:
  tool identity, calls, specs, results, execution contracts, and tool-batch orchestration
- `src/turn`:
  turn lifecycle events, item classes, and server request structures

## How To Extend This Crate

Add code here when the new concept answers one of these questions:

- Is this part of the stable language of a turn?
- Is this part of the durable language of a conversation?
- Is this part of the stable contract between the core agent and its tool system?
- Is this required for any frontend to reconstruct the same truth from stored facts?

If the answer is yes, it likely belongs in `agent-core`.

If the change is instead:

- a concrete tool implementation
- a transport adapter
- a storage engine
- a UI concern
- a product-specific workflow

then it likely belongs outside this crate.
