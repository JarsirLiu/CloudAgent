# agent-tools

`agent-tools` is the product-layer tool system for `cloudagent`.

This crate is responsible for:

- defining the default local tools exposed to the agent
- organizing tools by task domain rather than only by low-level capability
- selecting which tools should be visible for a given mode or task
- hosting concrete tool implementations and their product-facing descriptions

This crate is not responsible for the core tool protocol itself. Core tool abstractions such as
`ToolSpec`, `ToolCall`, `ToolResult`, and `ToolExecutor` belong in `agent-core`.

## Why This Crate Exists

The original tool system was intentionally thin: a single registry with a few primitive tools such
as directory listing, file reading, file writing, and shell execution.

That shape was easy to start with, but it has long-term weaknesses:

- repository analysis tends to degrade into directory-by-directory traversal
- the model must infer too much tool strategy from prompt text alone
- there is no stable place to express tool categories, task modes, or selection policy
- all tools are exposed at the same conceptual layer even when their use cases differ sharply

The v2 structure in this crate is intended to solve those weaknesses by separating:

- core protocol from product policy
- tool implementation from tool exposure
- task-oriented toolsets from raw file-system primitives

## Architectural Position

The tool system should be understood in three layers.

### 1. Core protocol layer

Owned by `agent-core`.

Examples:

- `ToolSpec`
- `ToolCall`
- `ToolResult`
- `ToolExecutionContext`
- `ToolExecutor`

This layer defines the language used by the runtime, the model adapter, and tool implementations.
It should remain stable and should not know the product's default tool catalog.

### 2. Product tool layer

Owned by `agent-tools`.

Examples:

- default local tool implementations
- tool descriptors
- tool categories
- tool risks
- tool mode tags
- toolset construction
- task and mode based tool selection

This layer is intentionally opinionated. It represents how `cloudagent` wants to package its tools
for real work.

### 3. Runtime orchestration layer

Owned by `agent-runtime`.

Examples:

- which task kind is currently active
- which tool mode should be used for this turn
- roundtrip guardrails
- approval routing
- tool call policy across multiple model turns

This layer decides when and how to expose subsets of the product tool layer to the model.

## Design Principles

The long-term tool system should follow these principles.

### Prefer task-oriented tools over primitive-only tools

The agent should not have to reconstruct repository analysis from repeated `list_dir` and
single-file reads. The default tool catalog should offer higher-information tools such as:

- text search
- file finding
- batch file reading
- patch-based editing

Primitive file-system operations still matter, but they should be supporting tools rather than the
default reasoning path.

### Separate stable abstractions from evolving strategy

The following belong in `agent-core` because they are stable protocol concepts:

- call and result types
- executor traits
- model-visible tool schema primitives

The following belong in `agent-tools` or `agent-runtime` because they are product strategy:

- which tools exist by default
- how tools are grouped into modes
- when repository analysis should prefer search over directory traversal
- which toolset is exposed in a given turn

### Make tool descriptions do real work

Long-term behavior should not rely only on a large persistent system prompt.

Each tool descriptor should help the model answer questions such as:

- when should this tool be chosen
- what is it better at than neighboring tools
- what should not be done with this tool

For example:

- repository search tools should say they are preferred for locating implementations
- directory listing tools should explicitly say they are not the primary discovery path
- batch read tools should say they exist to reduce model roundtrips

### Optimize for fewer model roundtrips

The main performance goal of the tool system is not simply "more tools". It is:

- higher information density per tool call
- fewer exploratory model turns
- earlier access to the actual implementation files

This is why repository exploration and batch reading are first-class concerns in v2.

### Make low-value search paths impossible by default

The agent should not waste time searching generated trees, vendored dependencies, or ignored
artifacts unless the user explicitly asks for them.

Repository exploration tools should therefore default to:

- respecting `.gitignore` when practical
- skipping common dependency directories such as `node_modules`
- skipping build output trees such as `dist`, `build`, `target`, and `.next`
- skipping cache and virtual-environment directories such as `.cache`, `.venv`, `venv`, and
  `__pycache__`

This behavior should live in the tool implementation and policy layer, not in the model prompt.
The model should not need to remember which junk directories to avoid.

### Keep mode selection outside the model when possible

The model should not be solely responsible for deciding which categories of tools are available.
That decision should be strongly shaped by the runtime and this crate's selection layer.

## Current v2 Skeleton

The current v2 layout is:

```text
src/v2/
├─ mod.rs
├─ spec/
│  └─ mod.rs
├─ selection/
│  └─ mod.rs
├─ policy/
│  └─ mod.rs
├─ registry/
│  └─ mod.rs
└─ impls/
   ├─ mod.rs
   ├─ repository_exploration/
   │  └─ mod.rs
   ├─ code_editing/
   │  └─ mod.rs
   ├─ command_execution/
   │  └─ mod.rs
   └─ workspace_file_ops/
      └─ mod.rs
```

These modules have distinct responsibilities.

### `spec`

Holds product-facing tool metadata:

- `ToolCategory`
- `ToolRisk`
- `ToolDescriptor`

This layer enriches the lower-level `ToolSpec` from `agent-core` with product semantics.

### `selection`

Holds selection concepts:

- `TaskKind`
- `ToolMode`
- `ToolSelector`

This layer answers the question: "Given the current task and mode, which tool descriptors should
be visible?"

### `policy`

Holds persistent strategy knobs and runtime-adjacent defaults such as:

- maximum directory-only exploration rounds
- whether batch reads should be encouraged
- the default exploration mode
- repository search defaults such as ignored directories and whether `.gitignore` should be
  respected

This is not yet wired into runtime enforcement, but this is where that policy belongs.

### `registry`

Holds `ToolRegistry`, which is the product registry for the default tool catalog.

Its responsibilities are:

- constructing the default descriptor list
- exposing all descriptors for inspection
- returning filtered `ToolSpec`s by `(ToolMode, TaskKind)`

Long-term, it should also own execution routing for v2 tools.

### `impls`

Holds concrete product tool families organized by task domain instead of only raw primitives.

Current domains:

- `repository_exploration`
- `code_editing`
- `command_execution`
- `workspace_file_ops`

Later domains may include:

- `external_resources`
- `agent_coordination`

## Tool Domains

The default long-term catalog should be organized around the following task domains.

### Repository exploration

Primary purpose:

- understand how the codebase works
- locate implementations
- gather evidence for architectural explanations

Primary tools:

- `search_text`
- `find_files`
- `read_file`
- `read_files`

Default repository exploration behavior should exclude ignored and generated trees unless a future
tool argument explicitly opts in.

This domain should become the default for questions like:

- "How does this mechanism work?"
- "Where is this feature implemented?"
- "What owns this workflow?"

### Code editing

Primary purpose:

- modify existing code safely
- create new files when patching is not appropriate

Primary tools:

- `apply_patch`
- `write_file`

Long-term, patch-oriented editing should be the default path for code changes.

### Command execution

Primary purpose:

- build
- test
- inspect system state
- use high-density shell workflows when appropriate

Primary tools:

- `shell_command`

This domain is important, but it should not be the only strategy for repository discovery.

### Workspace file operations

Primary purpose:

- confirm file-system structure
- inspect metadata
- perform targeted file-system operations

Primary tools:

- `get_metadata`
- `read_directory`

This domain is intentionally secondary for repository analysis.

## Default Tool Modes

The long-term runtime should expose different tool subsets depending on the task.

### Explore mode

Use for:

- codebase analysis
- architecture questions
- implementation discovery

Recommended visible tools:

- repository exploration tools
- selected command execution tools
- selected file metadata tools

### Edit mode

Use for:

- making code changes
- generating new files
- targeted follow-up inspection

Recommended visible tools:

- code editing tools
- read tools
- shell command for validation

### Verify mode

Use for:

- build
- test
- targeted re-checks

Recommended visible tools:

- shell command
- focused file reads
- metadata as needed

### Full mode

Use sparingly when the runtime explicitly wants the whole default catalog.

## What Must Move Out of the Persistent System Prompt

The persistent system prompt should hold only high-level behavior rules, for example:

- prefer high-information inspection
- batch independent calls when possible
- avoid repeated directory-only exploration

The following should not depend primarily on the global prompt:

- exact differences between repository search and directory listing
- when batch reads are preferred over single-file reads
- which tools belong to explore versus edit mode
- permission and deny-rule filtering

Those concerns belong in tool descriptors, selection, and runtime policy.

## Long-Term Implementation Roadmap

This crate should be implemented in stages.

### Phase 1: make repository exploration real

Implement the execution path for:

- `search_text`
- `find_files`
- `read_files`

Goal:

- replace directory-walk-heavy exploration with search-first discovery
- make search behavior respect ignore rules and skip low-value generated or dependency trees by
  default

Success metric:

- fewer model requests for architecture questions
- fewer consecutive directory-only rounds

### Phase 2: wire v2 registry execution

Teach `ToolRegistry` to:

- hold executable tool instances
- route `ToolCall`s by tool name
- return `ToolResult`s through the core protocol

At this stage, v2 becomes runnable instead of being descriptor-only.

### Phase 3: integrate with runtime selection

Update `agent-runtime` so it can request:

- `specs_for_mode(ToolMode::Explore, TaskKind::RepositoryAnalysis)`
- `specs_for_mode(ToolMode::Edit, TaskKind::CodeEdit)`

The runtime should stop exposing all tools all the time.

### Phase 4: move editing to patch-first workflows

Implement and prefer:

- patch-based editing
- smaller write surface area
- explicit code change reporting

### Phase 5: add runtime guardrails

Examples:

- cap directory-only exploration rounds
- prefer batch reads when multiple candidate files are obvious
- reject or rewrite known low-value exploration loops

## Migration Strategy

The current v1 registry remains in place during transition.

That is intentional.

Recommended migration order:

1. keep v1 operational
2. implement repository exploration tools in v2
3. allow selected runtime paths to use v2 explore mode
4. validate that logs show fewer roundtrips
5. migrate editing paths
6. retire v1 only after v2 is complete enough

This avoids a large unstable cutover.

## What Belongs Here vs Elsewhere

Use this rule when deciding where new code should live.

Put it in `agent-core` if:

- it defines a stable tool protocol concept
- it does not depend on the default tool catalog
- it should remain valid across multiple tool product strategies

Put it in `agent-tools` if:

- it defines a concrete default tool
- it describes how a tool should be presented to the model
- it groups or selects tools by task domain
- it encodes product-level strategy for tool exposure

Put it in `agent-runtime` if:

- it decides which tool mode to use during a turn
- it enforces roundtrip guardrails
- it coordinates approvals, cancellation, and tool availability over time

## Immediate Development Rule

From this point forward, new default local tools should not be added directly as flat v1-style
primitives unless there is a strong short-term reason.

New tool work should prefer the v2 structure and should answer these questions before
implementation:

1. Which task domain does the tool belong to?
2. Which mode tags should it carry?
3. What existing low-level behavior does it replace or reduce?
4. Does it increase information density per model turn?
5. Is it a stable product tool, or just a temporary bridge?

## Near-Term Next Steps

The next implementation work in this crate should be:

1. implement `search_text`
2. implement `find_files`
3. implement `read_files`
4. teach `ToolRegistry` to execute calls
5. connect runtime explore mode to v2 selection

That sequence creates the first real end-to-end improvement without forcing a full rewrite in one
pass.
