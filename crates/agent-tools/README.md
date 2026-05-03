# agent-tools

`agent-tools` owns the default local tool system for `cloudagent`.

This crate is where we define:

- which built-in tools exist
- how they are described to the model
- what capability family each tool belongs to
- what permission tier a tool requires
- whether a tool can safely participate in parallel execution

`agent-tools` should stay opinionated and compact. Its job is not to expose every possible helper.
Its job is to keep the main tool chain small, strong, and easy to reason about.

## Responsibilities

This crate is responsible for:

- product-facing tool descriptors
- shared local implementations for repository exploration, file changes, and command execution
- tool selection metadata such as mode tags and task tags
- permission visibility metadata
- execution strategy metadata such as parallel-safety

## Non-Responsibilities

This crate is not responsible for:

- turn orchestration
- approval request lifecycles
- conversation history management
- model request assembly
- UI event rendering

Those concerns belong elsewhere even when they consume tool metadata from this crate.

## Design Rules

- Keep the default tool surface small.
- Prefer stronger tools over many overlapping tools.
- Keep the primary tool chain concentrated in this crate.
- Put policy hints in tool metadata, not in scattered runtime conditionals.
- Put shared repository behavior behind reusable helpers.
- Prefer predictable output and stable schemas over clever but fragile formatting.

## Directory Guide

- `src/impls`: concrete built-in tool implementations
- `src/policy`: shared tool-level defaults and exploration policy
- `src/registry`: tool registration and dispatch wiring
- `src/selection`: surface selection and catalog filtering
- `src/spec`: tool descriptor metadata shared across the crate

## Boundary With Other Crates

- `agent-core` defines stable protocol types such as `ToolSpec`, `ToolCall`, and `ToolResult`.
- `agent-tools` builds the concrete local catalog on top of those protocol types.
- `agent-runtime` asks this crate for a resolved tool surface, then orchestrates turns around it.
