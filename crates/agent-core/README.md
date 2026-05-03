# agent-core

`agent-core` defines the stable agent domain model shared by higher-level crates.

This crate should stay narrow and durable. It is the place for protocol-like concepts and context
assembly helpers, not product-specific tool catalog choices or runtime turn orchestration.

## Responsibilities

- conversation and response item structures
- tool protocol types such as specs, calls, results, and execution context
- context preparation, budgeting, and compaction helpers
- model request and response domain types
- rollout and turn item representations

## Non-Responsibilities

- choosing the default local tool catalog
- approval UI and session interactions
- runtime event loops
- storage backends

## Directory Guide

- `src/context`: model-input preparation and history shaping
- `src/conversation`: conversation history primitives
- `src/model`: request and response domain types
- `src/projection`: lightweight derived views
- `src/rollout`: persisted turn-item representations
- `src/tool`: stable tool protocol types
- `src/turn`: item and delta types used across turns
