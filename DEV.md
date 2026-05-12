# Development Notes

## Current Shape

CloudAgent now has four concrete runtime roles:

- `cloudagent`: product entrypoint
- `cli`: terminal surface
- `node`: resident local host
- `agentd`: worker process

The repository is a Rust workspace with these top-level areas:

```text
apps/      executable entrypoints
cli/       reusable terminal surface crate plus cli binary
crates/    reusable Rust crates
configs/   configuration examples
docs/      current architecture docs
packaging/ release packaging assets
scripts/   install, upgrade, and validation helpers
tests/     workspace-level tests
web/       future web work
```

## Runtime Boundary

The intended runtime chain is:

```text
surface (cli / future web / IM)
  -> remote app-server client
  -> node
  -> worker(agentd)
  -> core
```

Key rule:

- surfaces do not talk to `agent-core` directly
- `node` owns resident lifecycle, worker reuse, platform runtime management, and transport hosting
- `agentd` owns execution of a worker session

## Executable Roles

### `apps/cloudagent`

Product-level command entrypoint.

Owns:

- `start/status/stop`
- platform management commands
- launching the CLI surface
- release-facing command UX

Should not own:

- terminal rendering internals
- worker protocol details

### `cli`

Terminal surface crate and `cli` binary.

Owns:

- console rendering
- terminal interaction
- local console bootstrap helpers

Should not own:

- product lifecycle command routing
- packaging concerns

### `apps/node`

Resident local host.

Owns:

- remote app-server host
- platform runtime lifecycle
- conversation registry/state
- worker spawning, reuse, and idle recycling

### `apps/agentd`

Worker-oriented binary.

Owns:

- stdio worker host
- embedded development console mode only where explicitly needed

## Crate Boundaries

### `agent-core`

Owns core conversation, turn, context, tool execution, approval, and orchestration semantics.

### `agent-app-server`

Owns app-server command routing, projection, session state, and server-request coordination.

### `agent-app-server-client`

Owns shared in-process/remote client access to the app-server protocol.

All surfaces should reuse this crate instead of inventing parallel client implementations.

### `agent-gateway`

Owns IM adapter logic and gateway-facing abstractions.

Current rule:

- IM platform code lives under `crates/agent-gateway/src/adapter/`
- platform adapters route back through `AppServerClient::Remote -> node`

### `agent-model-provider`

Owns provider-specific model execution adapters.

### `agent-memory`

Owns memory-facing abstractions and supporting logic used by the agent runtime.

### `agent-scheduler`

Owns scheduling-related abstractions and future recurring execution support.

### `agent-tools`

Owns tool definitions, registry logic, and workspace/system tool implementations.

### `config`

Owns workspace and runtime configuration loading.

### `infra-*`

Own low-level infrastructure integrations only:

- `infra-http`
- `infra-shell`
- `infra-ssh`
- `infra-store`

### `shared`

Owns lightweight common helpers and shared types.

## Cleanliness Rules

1. Keep product entry, surface logic, resident node logic, and worker logic separate.
2. Do not let surfaces bypass `agent-app-server-client`.
3. Do not let IM adapters define their own parallel node protocol.
4. Keep platform-specific code inside `agent-gateway/src/adapter/<platform>/`.
5. Prefer moving duplicated bootstrap/entry logic into shared library modules before adding new binaries.
6. Treat docs as part of the architecture surface; delete stale migration docs instead of half-maintaining them.

## Local Validation

Primary checks:

```bash
cargo fmt --all -- --check
cargo check --workspace --all-targets
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace --all-targets --no-fail-fast
```

Helpers:

- `scripts/ci-check.sh`
- `scripts/ci-check.ps1`

Use the PowerShell script on Windows when `bash` or WSL environment access to `cargo` is unreliable.
