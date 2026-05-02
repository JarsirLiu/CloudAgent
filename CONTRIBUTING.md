# Contributing Guide

## Commit Message Convention

We use **Conventional Commits**:

`<type>(<scope>): <subject>`

Examples:

- `feat(agent-tools): add fs/readFile alias`
- `fix(agent-runtime): prevent duplicate tool output projection`
- `docs(tooling): clarify commit message rules`
- `refactor(agent-tools): split repo tools into per-file modules`

### 1) Type

Allowed types:

- `feat`: new feature
- `fix`: bug fix
- `refactor`: code restructuring without behavior change
- `docs`: documentation only
- `test`: tests added/updated
- `chore`: maintenance/build/config changes
- `perf`: performance improvement
- `ci`: CI workflow change
- `revert`: revert a previous commit

### 2) Scope

Scope is required and should be a stable module/domain name.

Recommended scopes in this repo:

- `agent-tools`
- `agent-runtime`
- `agent-core`
- `agent-memory`
- `agent-protocol`
- `cli`
- `docs`
- `infra-shell`
- `infra-store`

If one commit touches multiple areas, prefer the dominant scope.

### 3) Subject

Subject rules:

- imperative mood (e.g. `add`, `fix`, `remove`, `rename`)
- lowercase start preferred
- no trailing period
- concise, ideally within 72 characters

Good:

- `fix(agent-tools): skip node_modules during search`

Bad:

- `fixed bug`
- `feat: Added New Things.`

### 4) Breaking Changes

For breaking changes:

- append `!` after type/scope, e.g. `feat(agent-tools)!: rename tool schema`
- describe migration details in commit body

### 5) Commit Body (when needed)

Add a body when context matters:

- what changed
- why it changed
- migration or compatibility notes

Example:

```text
refactor(agent-tools): rename repository groups to fs/repo namespaces

- move impls/workspace_file_ops to impls/fs
- move impls/repository_exploration to impls/repo
- keep runtime behavior unchanged
```

## Pull Request Checklist

- commit messages follow this guide
- tests pass locally (`cargo test --workspace`)
- no unrelated files included
- docs updated if behavior or API changed
