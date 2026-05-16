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
cloudagent start
cloudagent cli
cloudagent status
cloudagent stop
cloudagent upgrade
cloudagent uninstall
```

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
