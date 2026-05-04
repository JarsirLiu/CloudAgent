# agent-tools

`agent-tools` contains the built-in tool system for `cloudagent`.

This crate is the home of the default tool catalog, tool descriptors, tool routing metadata, and
the shared execution model used by local tools. The goal is to keep the tool chain small, strong,
and predictable so the agent can find code, inspect code, run commands, and make changes without
having to reason through a large number of overlapping helpers.

The tool system exists to reduce agent decision cost. A good tool system does not merely expose
many capabilities; it shapes those capabilities so the model can move from question to evidence to
action in a small number of steps. The design standard in this crate is therefore driven by
execution clarity, stable routing, and structured evidence density rather than by tool count.

## What This Crate Owns

- built-in tool descriptors exposed to the model
- shared implementations for repository exploration, file reads, file mutation, and command execution
- tool surface resolution and permission-aware visibility
- execution metadata such as parallel-safety and tool risk
- the canonical invocation and result model for tool capabilities

## Tool System Design

The tool system is organized around a single stable shape:

- descriptor:
  the model-visible schema, capability category, permission tier, and execution metadata
- invocation:
  the typed call envelope and typed payload passed into tool execution
- implementation:
  the concrete logic for a capability family such as workspace search or command execution
- result:
  the structured fact payload returned from tool execution
- projection:
  the downstream mapping used by runtime, history, protocol, and UI layers

This keeps local tools and future external tool sources aligned behind one execution model instead
of creating separate systems for each new capability family.

In practice, that means the tool system is designed around one backbone:

- one catalog of model-visible tools
- one invocation model for calling tools
- one routing layer that decides where a call goes
- one execution model per capability family
- one structured result model used as the shared fact source

New capability families should join this backbone. They should not create parallel tool systems,
shadow registries, or one-off result formats.

## Tool Design Philosophy

The design philosophy is simple:

- tools should help the model spend fewer steps getting to evidence
- tools should expose evidence in a structured way so later steps do not need to rediscover it
- tools should make the next correct action obvious
- tools should prefer a few strong capability families over many overlapping wrappers

This leads to several practical rules.

### 1. Tools are capability families, not convenience commands

A strong tool represents a durable capability family such as:

- workspace search
- file reading
- metadata lookup
- command execution
- file editing

A weak tool is a thin wrapper around a single shell habit or a narrow convenience action that the
model must chain with many siblings to finish basic work.

The preferred direction is therefore:

- merge overlapping tools into stronger structured entries
- keep narrow helpers out of the default catalog unless they create clear step reduction

### 2. Search must return evidence, not only text

Search is one of the highest leverage tool families in a coding agent. If search results are too
textual or too weakly structured, the model must repeat search, read, and filter steps to rebuild
the same facts.

Search tools in this crate should therefore aim to provide:

- clear search mode and scope
- stable session reuse when refinement is needed
- result counts and truncation metadata
- enough structured evidence for the next read or edit step to be chosen reliably

The success metric is not "search worked"; it is "the next tool call became obvious".

### 3. Reads should confirm facts, not force re-reading

Read tools should make it clear:

- which files were read
- which portions were read
- whether content was truncated
- whether another read is still needed

If the model cannot tell what it already saw, it will keep issuing extra read calls. Read tools are
therefore expected to support fact confirmation and comparison, not just raw file dumping.

### 4. Execution is a first-class runtime surface

Command execution is not just an escape hatch. It is a capability family used for build, test,
runtime inspection, and environment verification.

Because of that, execution tools should be designed with:

- explicit approval and risk metadata
- clear distinction between one-shot execution and session reuse
- structured running, completed, failed, and declined states
- stable output handling for history, logging, and UI projection

The model should not need to infer process state from incidental text.

### 5. Edits are protocol work, not text tricks

File modification should flow through an editing capability with explicit structure, validation, and
clear changed-file reporting. The edit surface is responsible for stable mutation semantics; it
should not rely on ad hoc command execution when a structured edit path exists.

### 6. Structured results are the fact source

Tool text is still useful, but it is not the system of record. The durable fact source is the
structured result payload.

That rule exists so the same tool result can be consumed consistently by:

- runtime event handling
- conversation history
- rollout and audit logs
- context filtering and compaction
- transcript projection
- UI rendering

If a result matters to later reasoning, it belongs in structured fields.

### 7. Skills are not tools

Skills help shape context, instructions, and workflow guidance. They do not belong in the callable
tool catalog.

This distinction matters because skills and tools solve different problems:

- skills change how the agent thinks
- tools change what the agent can execute

Mixing the two makes routing, logging, and safety harder to reason about.

### 8. External tool sources should reuse the same backbone

Future MCP-backed tools belong in this crate's tool system, but they should enter through the same
core concepts:

- descriptor
- invocation
- routing
- execution
- structured result

External does not mean special. It means a different source behind the same contract.

## Design Principles

- Keep the default tool surface small and high leverage.
- Prefer stronger structured tools over many narrow wrappers.
- Keep routing and tool metadata concentrated in this crate.
- Treat structured results as the fact source and display text as a projection.
- Make permission, risk, and execution behavior explicit in tool metadata.
- Add new capability families by extending the existing tool model instead of bypassing it.

When these principles conflict, favor the option that reduces repeated search, repeated reads, and
repeated command retries for common coding tasks.

## What Counts As A Tool

The following belong here:

- repository exploration
- structured file reads
- structured file mutation
- command execution
- future MCP-backed callable tools

The following do not belong here:

- turn orchestration
- conversation history management
- UI rendering
- skill loading and skill-context injection
- long-term memory loading

Skills are part of context construction, not part of the tool catalog. MCP tools are part of the
tool catalog, but they should reuse the same invocation, routing, and result model as built-in
tools.

## Code Organization

The crate is split into a few focused modules:

- [`src/impls`](./src/impls) contains concrete built-in tool implementations.
- [`src/spec`](./src/spec) defines descriptor metadata shared across the tool catalog.
- [`src/selection`](./src/selection) resolves tool surfaces and catalog filtering.
- [`src/policy`](./src/policy) contains shared tool-level policy and approval helpers.
- [`src/registry`](./src/registry) wires descriptors, routing, presentation, and dispatch together.

## Boundary With Other Crates

- `agent-core` defines the stable shared contracts such as `ToolSpec`, `ToolCall`, `ToolResult`,
  and `ToolSurface`.
- `agent-tools` builds the concrete tool catalog and execution model on top of those contracts.
- `agent-runtime` resolves a tool surface from this crate and orchestrates turns around it.

If you are extending the tool system, start in this crate and keep new capability work inside the
existing descriptor, invocation, execution, and result flow.
