# CloudAgent Release Installation

CloudAgent release installs are script-first and archive-backed.

After installation, the product entrypoint is always:

```bash
cloudagent
```

You should be able to use these commands immediately:

```bash
cloudagent start
cloudagent cli
cloudagent status
cloudagent stop
cloudagent upgrade
```

`cloudagent cli` exits only the CLI surface. It does not stop the local node.

## Linux / macOS

Install:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.sh | sh
```

Upgrade:

```bash
cloudagent upgrade
```

Uninstall:

```bash
cloudagent uninstall
```

Default locations:

- Data: `~/.cloudagent`
- Installed binaries: `~/.local/lib/cloudagent/current`
- Launchers: `~/.local/bin`

`cloudagent uninstall` keeps `~/.cloudagent` by default. To delete user data too:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh -s -- --purge
```

## Windows

Install:

```powershell
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/install.ps1 | iex
```

Upgrade:

```powershell
cloudagent upgrade
```

Uninstall:

```powershell
cloudagent uninstall
```

Default locations:

- Data: `%USERPROFILE%\.cloudagent`
- Installed binaries: `%LOCALAPPDATA%\CloudAgent\current`
- Launcher: `%USERPROFILE%\.local\bin\cloudagent.cmd`

`cloudagent uninstall` keeps `%USERPROFILE%\.cloudagent` by default. To delete user data too:

```powershell
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.ps1 | iex
```

Then run:

```powershell
& "$env:USERPROFILE\.local\bin\cloudagent.cmd" uninstall --purge
```
