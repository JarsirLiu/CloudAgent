# Repository Access Transition Notes

This directory holds the repository-facing building blocks that support the compact core tool
stack.

## Final Direction

Repository analysis should center on these capabilities:

- `fuzzy_file_search` for candidate file discovery
- `fs_read_file` for reliable file reading
- `shell_command` for high-fidelity text search via `rg`, `git`, and other native workflows

These are the primary capabilities in this area. Shared helpers inside this directory should serve
those tools directly.

## What This Directory Is For

Right now this directory is the best place to concentrate shared repository-facing behavior such
as:

- ignore-aware walking
- path normalization
- text decoding and truncation
- file-match ranking
- search helpers shared by the core repo tools

Those shared pieces are useful even if some current tool names disappear later.

### `fuzzy_file_search`

`fuzzy_file_search` is the file-discovery entry point. Its job is to return the most likely
candidate files quickly in large repositories.

### `fs_read_file`

`fs_read_file` is the file-reading entry point. Its job is to read known files reliably with
centralized handling for:

- truncation
- binary detection
- encoding fallback
- path safety

## Design Principles

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

### Keep outputs model-readable

Claude Code remains a useful reference for single-tool design. Tools here should still aim
for:

- strict schemas
- good defaults
- concise summaries
- predictable output formatting

## Near-Term Priorities

Work in this directory should favor the following sequence:

1. strengthen shared repo helpers
2. improve `fs_read_file` toward a reusable shared file access path
3. improve `fuzzy_file_search` toward stronger ranking and search behavior
4. strengthen shell-first native search workflows where practical
5. remove duplicated logic that would block the final consolidation

## What Not To Do

Avoid using this directory for:

- many small one-off repo tools
- separate read variants that duplicate each other
- directory-walk-first exploration strategies
- redundant public tool names that do not improve the core stack

The point of this module family is to support the stable final tool stack, not to grow the surface
area.
