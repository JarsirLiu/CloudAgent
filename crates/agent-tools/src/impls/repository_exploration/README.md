# Repository Exploration Tools

This module family is the long-term home for repository-analysis-first tools.

The first tool to implement here should be `search_text`.

## Reference Mapping

When borrowing ideas from other agents, the recommended mapping is:

- `search_text` -> Claude Code `GrepTool`
- `find_files` -> Claude Code `GlobTool`
- `read_file` -> Claude Code `FileReadTool`
- `read_files` -> local enhancement for reducing roundtrips

For concrete single-tool design, prefer Claude Code as the reference.
For system-level tool exposure and orchestration, prefer Codex as the reference.

## Why `search_text` Comes First

The old tool path encouraged this pattern:

- list a directory
- list a child directory
- list another child directory
- read one file

That creates too many model roundtrips and wastes context on structure discovery.

`search_text` should become the default implementation-locating tool for questions such as:

- where is this mechanism implemented
- which files mention this symbol
- what calls this function or type
- where does this workflow begin

## Claude Code Design Lessons

The most important ideas to copy from Claude Code `GrepTool` are:

### 1. The tool has a strong opinionated description

Claude Code explicitly tells the model to use `GrepTool` for search instead of shelling out to
`grep` or `rg`.

For `cloudagent`, `search_text` should similarly state:

- use this tool first for implementation discovery
- prefer this over repeated directory walking
- do not use generic shell search unless there is a task-specific reason

### 2. The tool has a strict input schema

Claude Code does not accept vague free-form search input. It strongly models search parameters.

`search_text` should do the same in Rust.

The first version does not need every advanced option, but it should still use a typed argument
model instead of raw ad hoc JSON handling.

### 3. The tool applies safe defaults

Claude Code limits result volume and excludes noisy directories.

`search_text` should default to:

- respecting `.gitignore` when practical
- excluding common dependency and build output directories
- truncating result count by default
- returning relative paths when possible to save tokens

### 4. The tool returns structured output

Claude Code tools do not only dump text. They expose structured fields that are easier to reason
about in later layers.

`search_text` should return:

- total match count
- matched file count
- whether output was truncated
- the actual match entries

### 5. The tool gives the model a concise textual summary

Even with structured output, the model still benefits from a compact human-readable summary.

That summary should say things like:

- found 12 matches in 4 files
- showing first 8 matches

Then list the top hits in a predictable format.

## First-Version Contract for `search_text`

The first version should intentionally stay narrower than Claude Code `GrepTool`, but the shape
should be compatible with future growth.

### Tool name

- `search_text`

### Primary purpose

- locate implementations by keyword or simple pattern
- reduce exploratory directory traversal

### First-version input arguments

- `query: string`
- `path_scope?: string`
- `max_results?: integer`

The first version may postpone:

- regex mode
- glob filtering
- multiline matching
- type filtering
- pagination

Those can be added after the core execution path is stable.

### First-version output structure

- `match_count: usize`
- `file_count: usize`
- `truncated: bool`
- `results: SearchTextMatch[]`

Where each `SearchTextMatch` contains:

- `path: String`
- `line: usize`
- `preview: String`

### Suggested textual result shape

```text
Found 9 matches in 3 files.
Showing first 9 matches.

crates/agent-runtime/src/tasks/regular.rs:126: if let Some(compaction_plan) = ...
crates/agent-memory/src/lib.rs:42: pub struct MemoryCompactor ...
...
```

## Search Defaults

`search_text` should not rely on the model to remember junk directories.

The implementation should default to excluding at least:

- `.git`
- `.hg`
- `.svn`
- `node_modules`
- `dist`
- `build`
- `target`
- `target-verify`
- `.next`
- `.nuxt`
- `.turbo`
- `.cache`
- `coverage`
- `.venv`
- `venv`
- `__pycache__`

Long-term behavior should also respect `.gitignore` when practical.

If a future task really needs to search ignored content, that should require explicit opt-in via
tool arguments rather than being the default path.

## Suggested Rust Shape

The Rust implementation should eventually be split into the same conceptual pieces used by strong
Claude-style tools:

- `SearchTextArgs`
- `SearchTextMatch`
- `SearchTextOutput`
- `descriptor()`
- `validate(...)`
- `run(...)`
- `render_summary(...)`

This makes the tool easier to evolve without turning it into a single large function.

## Non-Goals for the First Version

The first version does not need to:

- match every `ripgrep` feature
- handle every encoding edge case
- support arbitrary binary inspection
- replace shell command usage for advanced search workflows

It only needs to be the default high-value search tool for repository analysis.

## Implementation Standard

Before `search_text` is considered complete, it should satisfy all of the following:

1. The schema is typed and validated.
2. The tool skips ignored and generated trees by default.
3. The result volume is capped by default.
4. The output is both structured and model-readable.
5. The tool is clearly better than directory walking for implementation discovery.

If it does not satisfy those conditions, it is not yet good enough to replace the old exploration
path.
