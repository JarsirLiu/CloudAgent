# agent-runtime

`agent-runtime` orchestrates turns.

This crate is responsible for executing the live agent loop around the stable core types and the
built-in tool catalog. It should coordinate work, not become the home for per-tool hardcoded
business rules.

## Responsibilities

- running turn lifecycles
- streaming model responses
- coordinating tool batches
- handling approvals and interruption
- recording rollout events
- bridging between model adapters, tool registry, and stored state

## Non-Responsibilities

- defining the built-in tool catalog
- owning stable protocol structures
- carrying duplicated per-tool visibility or parallel-safety lists

## Directory Guide

- `src/engine`: model and turn execution adapters
- `src/state`: runtime state and active-turn coordination
- `src/tasks`: high-level task entry points such as regular turns and manual compaction
- `src/tools`: runtime-side tool orchestration helpers
