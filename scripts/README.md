# CloudAgent Release Scripts

CloudAgent release installation is managed by the scripts in this directory.

Release process standard: [docs/release-process.md](../docs/release-process.md)

Current note:

- the remote installer entrypoints in this directory are bootstrap and recovery
  paths
- the target steady-state design is local-first upgrade using installed support
  logic plus release metadata and archives

Release entry:

- [CloudAgent Releases](https://github.com/JarsirLiu/CloudAgent/releases)

## Install

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh | sh
```

Windows:

```bash
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.ps1 | iex
```

You can also download the installer scripts first and run them locally:

Linux / macOS:

```bash
curl -fsSLO https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh
sh install.sh
```

Windows:

```bash
Invoke-WebRequest https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.ps1 -OutFile install.ps1
powershell -ExecutionPolicy Bypass -File .\install.ps1
```

## Runtime Commands

After installation, the product entrypoint is:

```bash
cloudagent
```

Common commands:

```bash
# show the top-level help
cloudagent

cloudagent start
cloudagent cli
cloudagent status
cloudagent stop
cloudagent upgrade
cloudagent uninstall
```

`cloudagent` with no arguments shows the top-level help. `cloudagent cli` opens the interactive CLI surface. Unknown commands such as `cloudagent xxx` now fail fast with an "unknown command" error instead of falling back to the CLI.

`cloudagent cli` exits only the CLI surface. It does not stop the local node.

## Install Locations

Linux / macOS:

- Data directory: `~/.cloudagent`
- Installed binaries: `~/.local/share/cloudagent/current`
- Launchers: `~/.local/bin`

Windows:

- Data directory: `%USERPROFILE%\\.cloudagent`
- Installed binaries: `%LOCALAPPDATA%\\CloudAgent\\current`
- Launcher: `%USERPROFILE%\\.local\\bin\\cloudagent.cmd`

## Upgrade And Uninstall

Normal upgrade is moving toward a local-first model. During migration, some
bridge behavior may still exist, but the target contract is:

- `cloudagent upgrade` uses installed local upgrade logic first
- network access is limited to release metadata and release archives
- remote script execution is transitional rather than the long-term contract

Release version handling is shared across install, upgrade, CI, and release publishing:

- Shell scripts use [`release_tag_rules.sh`](./release_tag_rules.sh)
- PowerShell scripts use [`release-tag-rules.ps1`](./release-tag-rules.ps1)
- The validation wrapper is [`validate-release-tag.ps1`](./validate-release-tag.ps1)

The helper self-tests are wired into CI, so tag rule changes should be made in the shared rule files rather than copied into each script.

Installer scripts also expose `--self-test` / `-SelfTest` smoke checks, and CI runs them from temporary directories that do not contain the shared helper files.

Release publishing follows a staged flow:

- Build jobs produce versioned release archives
- The publish job collects artifacts into a staging directory and writes `SHA256SUMS`
- GitHub Release notes come from the tag commit message

That split keeps release artifacts reproducible and makes installer updates easier to validate before they reach users.

For the canonical release policy, install/upgrade/rollback behavior, beta handling, and both uninstall mechanisms, see [docs/release-process.md](../docs/release-process.md).

Install and upgrade downloads now show terminal-friendly progress:

- PowerShell, Windows Terminal, and `cmd` launched through the PowerShell installer show `MB / total MB` progress
- Linux and macOS interactive terminals show a `curl` progress bar
- Non-interactive environments fall back to quieter output

Uninstall keeps user data by default. Use `cloudagent uninstall --purge` to delete user data too.
