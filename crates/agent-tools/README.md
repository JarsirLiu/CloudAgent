# agent-tools

`agent-tools` is the product-facing tool layer for `cloudagent`.

This crate should not grow into a catalog of many mediocre tools. Its job is to expose a small,
stable, well-described set of high-value capabilities that the agent can rely on across large real
world repositories.

The current direction is:

- align core tool architecture with Codex
- absorb strong single-tool design ideas from Claude Code
- remove low-value exploration loops
- prefer fewer, more reliable tools over a larger tool surface

## Tool Philosophy

The long-term goal is not "more tools". The goal is:

- stable behavior
- high information density per call
- predictable outputs
- strong Windows and large-repo behavior
- low model roundtrip count

That means `cloudagent` should converge on a compact core toolset instead of maintaining many
partially overlapping primitives.

## Reference Strategy

Two reference systems matter here, but they should influence different layers.

### Codex as the architecture reference

Codex is the primary reference for:

- keeping the exposed tool surface small
- building around core capabilities instead of many ad hoc tools
- using shell workflows where they are the highest-fidelity path
- treating file search and file reading as infrastructure, not one-off helpers
- consolidating editing around a patch-first workflow

### Claude Code as the single-tool UX reference

Claude Code is the secondary reference for:

- strong tool descriptions
- strict schemas
- safe defaults
- concise model-readable summaries
- clarifying when a tool should and should not be used

In short:

- architecture and tool catalog shape should lean Codex
- per-tool ergonomics should learn from Claude Code

## Target Core Toolset

The default tool stack should converge on a very small set of core capabilities.

### 1. `shell_command`

Primary purpose:

- build
- test
- inspect runtime state
- run high-fidelity repo search workflows such as `rg`, `git`, and other real shell commands

Long-term expectation:

- this is the preferred path for advanced text search
- command safety, approval policy, and platform behavior must be robust

### 2. `apply_patch`

Primary purpose:

- make code changes through a single patch-first editing path

Long-term expectation:

- this becomes the primary editing tool
- other editing entry points should converge into this workflow or disappear

### 3. `fs_read_file`

Primary purpose:

- read known files reliably
- support targeted inspection after file discovery

Long-term expectation:

- this should be backed by a shared file-reading layer
- binary detection, truncation, encoding fallback, and path safety should be centralized

### 4. `fuzzy_file_search`

Primary purpose:

- locate candidate files quickly in large repositories

Long-term expectation:

- this should evolve toward a Codex-style fuzzy file search capability
- file discovery should not depend on repeated directory walking

### 5. `fs_stat`

Primary purpose:

- answer focused metadata questions cheaply

Long-term expectation:

- this remains a narrow helper, not a primary exploration path

## Tools That Are Transitional, Not Final

Several existing tools are useful during migration, but they should not define the long-term
product shape.

- `read_files`
- `read_directory`
- `write_file`
- `edit_file`
- the current transitional `find_files`
- the current transitional `search_text`

These may continue to exist for compatibility while the new core toolset is built out, but they
should be treated as bridges rather than permanent product commitments.

## Architectural Layers

The tool system should be understood in three layers.

### 1. Core protocol layer

Owned by `agent-core`.

Examples:

- `ToolSpec`
- `ToolCall`
- `ToolResult`
- `ToolExecutionContext`
- `ToolExecutor`

This layer defines stable protocol concepts and must remain independent of the default product
catalog.

### 2. Product tool layer

Owned by `agent-tools`.

Responsibilities:

- define the default local tool catalog
- describe tools in a model-friendly way
- group shared file access, search, edit, and command behaviors
- encode product-level tool strategy

This layer should be opinionated, but it should stay compact.

### 3. Runtime orchestration layer

Owned by `agent-runtime`.

Responsibilities:

- decide which tools are exposed for a turn
- enforce approvals and guardrails
- coordinate cancellation and policy

This layer decides availability over time. It should not compensate for a bloated or low-quality
tool catalog.

## Design Rules

### Prefer infrastructure over ad hoc helpers

File reading, file discovery, and editing should be shared capabilities, not separate piles of
copy-pasted logic hidden inside many tools.

### Prefer shell for high-fidelity search

When the highest quality answer comes from `rg`, `git`, or another real command, the system should
lean on `shell_command` rather than forcing a weaker custom replica.

### Keep the tool surface small

Every exposed tool creates model choice complexity. A weak tool is worse than no tool.

### Make tool descriptions carry strategy

The model should learn key behavior from the tool descriptor itself:

- what the tool is best at
- what neighboring tools it should replace
- when not to use it

### Bake defaults into the implementation

The model should not need to remember:

- which directories are junk
- when to batch reads
- when to avoid directory-only exploration

Those defaults belong in the implementation and policy layers.

## Migration Direction

The migration target is not "finish v2 exactly as first imagined". The target is to reach a
Codex-shaped core tool stack as quickly as possible without sacrificing reliability.

Recommended order:

1. tighten documentation around the final target
2. strengthen the current transitional implementations only when they directly support the target
3. converge file reading into a shared `fs_read_file` path
4. converge editing into `apply_patch`
5. replace transitional file discovery with `fuzzy_file_search`
6. reduce or remove tools that no longer justify exposure

## What Belongs Here

Put code in `agent-tools` if:

- it defines a default local tool that should remain part of the product
- it implements shared tool-facing behavior for reading, searching, editing, or command execution
- it improves model-facing tool descriptions and strategy

Put code elsewhere if:

- it is only a stable protocol concept
- it is runtime policy rather than tool behavior
- it is a one-off compatibility bridge that should be short-lived

## Immediate Development Rule

From this point forward, do not add new default tools just to fill gaps in the current transitional
surface.

Before adding or keeping a tool, answer:

1. Does this belong in the final compact core toolset?
2. Is it stronger than using one of the existing core capabilities?
3. Is it infrastructure-backed, or is it another ad hoc wrapper?
4. Will it reduce roundtrips and improve reliability in large repositories?

If the answer is no, the tool should probably not exist.
