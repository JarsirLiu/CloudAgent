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

- `fuzzy_file_search`
- `fs_read_file`

They may continue to evolve while the system is being stabilized, but they should not be mistaken
for the final public tool surface.

### `fuzzy_file_search`

`fuzzy_file_search` is still transitional at the implementation level.

Its long-term direction is to gain stronger ranking, session support, and large-repo behavior so
that file discovery feels closer to the Codex implementation.

### `fs_read_file`

`fs_read_file` is closer to a final capability, but the current implementation still needs to keep
moving toward a stronger shared file access path with centralized handling
for:

- truncation
- binary detection
- encoding fallback
- path safety

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

If `rg` can provide a better answer than a custom search helper, prefer `rg`.

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
2. improve `fs_read_file` toward a reusable shared file access path
3. improve `fuzzy_file_search` toward stronger ranking and session behavior
4. strengthen shell-first native search workflows where practical
5. remove duplicated logic that would block the final consolidation

## What Not To Do

Avoid using this directory as the long-term home for:

- many small one-off repo tools
- separate read variants that duplicate each other
- directory-walk-first exploration strategies
- product commitments to transitional tool names

The point of this module family is to help us get to a stable final tool stack faster, not to
freeze the current transition state in place.
