# CloudAgent Release Scripts

CloudAgent release installation is managed by the scripts in this directory.

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
- Installed binaries: `~/.local/lib/cloudagent/current`
- Launchers: `~/.local/bin`

Windows:

- Data directory: `%USERPROFILE%\\.cloudagent`
- Installed binaries: `%LOCALAPPDATA%\\CloudAgent\\current`
- Launcher: `%USERPROFILE%\\.local\\bin\\cloudagent.cmd`

## Upgrade And Uninstall

Upgrade:

```bash
cloudagent upgrade
```

Release version handling is shared across install, upgrade, CI, and release publishing:

- Shell scripts use [`release_tag_rules.sh`](./release_tag_rules.sh)
- PowerShell scripts use [`release-tag-rules.ps1`](./release-tag-rules.ps1)
- The validation wrapper is [`validate-release-tag.ps1`](./validate-release-tag.ps1)

The helper self-tests are wired into CI, so tag rule changes should be made in the shared rule files rather than copied into each script.

The bootstrap release tree is staged by [`stage-bootstrap.sh`](./stage-bootstrap.sh) and validated before it is pushed to the `release-bootstrap` branch.

Installer scripts also expose `--self-test` / `-SelfTest` smoke checks, and CI runs them from temporary directories that do not contain the shared helper files.

Release publishing follows a staged flow:

- Build jobs produce versioned release archives
- The publish job collects artifacts into a staging directory and writes `SHA256SUMS`
- GitHub Release notes come from the tag commit message
- The `release-bootstrap` branch is updated from a staged bootstrap tree that carries the installer entrypoints and shared helpers

That split keeps release artifacts reproducible and makes installer updates easier to validate before they reach users.

Install and upgrade downloads now show terminal-friendly progress:

- PowerShell, Windows Terminal, and `cmd` launched through the PowerShell installer show `MB / total MB` progress
- Linux and macOS interactive terminals show a `curl` progress bar
- Non-interactive environments fall back to quieter output

Uninstall:

```bash
cloudagent uninstall
```

By default, uninstall keeps user data in the CloudAgent data directory.

To delete user data too:

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh -s -- --purge
```

Windows:

```bash
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.ps1 | iex
& "$env:USERPROFILE\.local\bin\cloudagent.cmd" uninstall --purge
```
