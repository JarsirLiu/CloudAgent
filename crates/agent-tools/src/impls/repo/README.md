# Repository Access Transition Notes

This directory currently holds transitional repository-access implementations.

It is not the final long-term tool shape.

The purpose of this module family is to bridge the gap between the older primitive exploration
path and a smaller, stronger tool stack aligned with Codex-style architecture.

## Final Direction

Long-term, repository analysis should converge on these capabilities:

- `fuzzy_file_search` for candidate file discovery
- `fs_read_file` for reliable file reading
- `shell_command` for high-fidelity text search via `rg`, `git`, and other native workflows

That means the current repo tools in this directory should be treated as migration helpers, not
permanent product commitments.

## What This Directory Is For

Right now this directory is the best place to concentrate shared repository-facing behavior such
as:

- ignore-aware walking
- path normalization
- text decoding and truncation
- file-match ranking
- transitional search helpers

Those shared pieces are useful even if some current tool names disappear later.

## What Is Transitional

The following current tools are transitional:

- `search_text`
- `find_files`
- `read_file`
- `read_files`

They may continue to evolve while the system is being stabilized, but they should not be mistaken
for the final public tool surface.

### `search_text`

`search_text` is currently a bridge tool.

Its job is to improve on directory-walk-heavy exploration while the system moves toward stronger
shell-first search behavior. It can continue to exist during migration, but it should not become
the long-term center of the architecture.

For high-fidelity search, the long-term preferred path is:

- `shell_command`
- `rg`
- `git grep` or similar native repo commands when appropriate

### `find_files`

`find_files` is also transitional.

Its long-term direction is not "better globbing". Its long-term direction is to evolve toward a
real `fuzzy_file_search` capability with better ranking, session support, and large-repo behavior.

### `read_file`

`read_file` is the closest transitional tool to a final capability, but even here the end state is
not a repo-local helper. The end state is a shared `fs_read_file` path with centralized handling
for:

- truncation
- binary detection
- encoding fallback
- path safety

### `read_files`

`read_files` should be treated as temporary.

Batch reads can still be useful during migration, but the long-term design should avoid exposing a
separate standalone read tool if the same outcome can be achieved through a stronger shared file
read layer and better model strategy.

## Design Principles for Transitional Work

Even while these tools are transitional, changes here should still follow strong rules.

### Prefer shared helpers over per-tool duplication

If a repo tool needs:

- text decoding
- ignore-aware walking
- truncation rules
- ranking rules

that behavior should live in shared helpers instead of being reimplemented in each tool.

### Prefer native search engines when possible

If `rg` can provide a better answer than a custom text scan, prefer `rg`.

Fallback logic is acceptable, but the highest-quality engine should be the primary path.

### Do not optimize transitional tools into permanent sprawl

It is fine to improve current tools when they directly support the migration target. It is not fine
to keep inventing adjacent repo tools that expand the long-term surface area.

### Keep outputs model-readable

Claude Code remains a useful reference for single-tool design. Transitional tools should still aim
for:

- strict schemas
- good defaults
- concise summaries
- predictable output formatting

## Near-Term Priorities

Work in this directory should favor the following sequence:

1. strengthen shared repo helpers
2. improve `read_file` toward a reusable `fs_read_file` implementation path
3. improve `find_files` toward `fuzzy_file_search`
4. let `search_text` use native search engines where practical
5. remove duplicated logic that would block the final consolidation

## What Not To Do

Avoid using this directory as the long-term home for:

- many small one-off repo tools
- separate read variants that duplicate each other
- directory-walk-first exploration strategies
- product commitments to transitional tool names

The point of this module family is to help us get to a stable final tool stack faster, not to
freeze the current transition state in place.
