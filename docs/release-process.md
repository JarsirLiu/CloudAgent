# CloudAgent Release Process

This document defines the standard release flow for CloudAgent.

It follows the same core ideas used by Codex:

- versioned releases are immutable once published
- installs and upgrades switch a `current` pointer instead of overwriting the active version in place
- beta and alpha builds are published as prereleases
- rollback is done by pointing `current` back to a previous version directory

## 1. Version Rules

- All release tags use a leading `v`, for example `v0.1.43`
- Stable releases use plain semantic version tags, for example `v0.1.43`
- Pre-release builds use semantic version suffixes, for example:
  - `v0.1.44-beta.1`
  - `v0.1.44-alpha.1`
- Build metadata such as `+build.7` is allowed if needed

Release tags are validated by the shared tag helpers:

- `scripts/release_tag_rules.sh`
- `scripts/release-tag-rules.ps1`
- `scripts/validate-release-tag.ps1`

## 2. Standard Release Flow

The normal release flow is:

1. Bump the workspace version to the next release tag.
2. Commit the version bump with a clear release-oriented message.
3. Create a tag that matches the version, for example `v0.1.43`.
4. Push the tag to GitHub.
5. GitHub Actions triggers the release workflow from the tag push.
6. The workflow builds binaries for all supported platforms.
7. The workflow generates `SHA256SUMS`.
8. The workflow publishes the GitHub Release and uploads assets.
9. The workflow publishes the release assets and leaves the installers to resolve scripts directly from GitHub Releases and the repository `main` branch.

For prerelease builds, use a tag such as `v0.1.44-beta.1`.
The release workflow treats tags containing `-beta` or `-alpha` as prereleases.

## 3. What Goes Into a Release

Each published release should include:

- platform archives for Linux, macOS, and Windows
- a checksum manifest
- release notes based on the tag commit message
- installer scripts served from the repository `main` branch

The installers and upgraders resolve the latest release metadata and helper scripts directly from GitHub Releases and the `main` branch.

## 4. Install Flow

CloudAgent installation should be predictable and recoverable.

The installer should:

1. Resolve the target version.
2. Download the release asset and checksum manifest from GitHub Releases.
3. Verify the archive checksum.
4. Extract into a staged version directory.
5. Switch `current` to the staged directory only after extraction succeeds.
6. Refresh launchers and PATH entries after the version switch succeeds.

This design keeps the active installation intact until the new version is ready.

## 5. Upgrade Flow

The upgrade flow should:

1. Detect whether the local node is running.
2. Stop the node before replacing binaries if needed.
3. Run the installer for the desired version.
4. Restart the node after a successful upgrade.

If upgrade fails before the final pointer switch, the previous installed version should remain usable.

## 6. Rollback Policy

CloudAgent should keep previous version directories on disk whenever possible.

That gives you two rollback paths:

- re-point `current` to the previous version directory
- reinstall a pinned older release with the installer

There is no need to overwrite the last known-good install during normal upgrades.

## 7. Beta And Prerelease Policy

Beta and alpha releases are first-class release channels.

Recommended rules:

- use `vX.Y.Z-beta.N` for public beta testing
- use `vX.Y.Z-alpha.N` for earlier validation builds
- publish those tags as GitHub prereleases
- keep stable releases separate from prereleases

This lets stable users stay on the latest stable release while testers opt into prerelease builds explicitly.

## 8. Uninstall Policy

CloudAgent keeps two uninstall mechanisms on purpose.

### 8.1 Product Command

Users can uninstall from the installed product entrypoint:

```bash
cloudagent uninstall
cloudagent uninstall --purge
```

Default behavior:

- remove launchers and installation files
- keep user data

`--purge` behavior:

- remove launchers and installation files
- remove user data as well

### 8.2 Direct Script Entry

Users can also run the standalone uninstall scripts directly.

Linux / macOS:

```bash
curl -fsSL https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.sh | sh -s -- --purge
```

Windows:

```powershell
irm https://raw.githubusercontent.com/JarsirLiu/CloudAgent/main/scripts/uninstall.ps1 | iex
& "$env:USERPROFILE\.local\bin\cloudagent.cmd" uninstall --purge
```

These two uninstall paths should stay documented and supported.

## 9. Documentation Sources

The main references are:

- `scripts/install.sh`
- `scripts/install.ps1`
- `scripts/upgrade.sh`
- `scripts/upgrade.ps1`
- `scripts/uninstall.sh`
- `scripts/uninstall.ps1`
- `scripts/stage-release-scripts.sh`
- `.github/workflows/release.yml`
