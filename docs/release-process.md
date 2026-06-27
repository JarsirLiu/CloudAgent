# CloudAgent Release Process

This document defines the target release, install, upgrade, mirror, rollback,
and migration strategy for CloudAgent across Windows, Linux, and macOS.

The goal is to keep installation and upgrades predictable for all platforms
while avoiding a fragile dependency on remotely executed installer scripts.

## 1. Goals

CloudAgent release and update behavior should satisfy all of the following:

- versioned releases are immutable once published
- installs and upgrades stage a new version before switching the active one
- rollback is possible by re-pointing `current` to a previous version
- Windows, Linux, and macOS follow the same release protocol
- the normal upgrade path depends on local installer logic plus remote metadata
  and release archives, not on downloading and executing remote helper scripts
- release distribution can move from GitHub to a mirror or CDN without forcing
  users to uninstall and reinstall
- prerelease channels remain first-class and explicit

## 2. Current Problem

The current implementation already uses versioned install directories and a
`current` pointer, which is good.

The weak point is the upgrade control plane:

- local launchers may download remote helper scripts
- helper scripts may in turn download other helper scripts
- version resolution, script distribution, and archive distribution are coupled
- changing download sources later becomes harder because old clients may not
  understand the new path

That model is too fragile for long-term, cross-platform, mirrored
distribution.

## 3. Target Architecture

CloudAgent should use a three-layer model:

1. Local bootstrap and installer logic
2. Remote release metadata
3. Remote release archives

The key rule is:

- normal `cloudagent upgrade` must execute local upgrade logic
- remote systems may provide metadata and archives
- remote systems must not be required to provide executable installer logic for
  the normal upgrade path

### 3.1 Local Bootstrap

Each installed CloudAgent must include local install and upgrade logic that can:

- resolve the desired release channel and version
- fetch release metadata
- choose the correct platform archive
- download and verify the archive
- unpack to a staged version directory
- atomically switch the active version pointer
- refresh visible command launchers after the switch

This local logic is allowed to evolve with each release and should be replaced
as part of a successful upgrade.

### 3.2 Remote Metadata

Clients should resolve release information from a stable metadata endpoint
instead of encoding GitHub-specific behavior into the launcher contract.

Examples:

- `https://downloads.example.com/cloudagent/channels/stable.json`
- `https://downloads.example.com/cloudagent/channels/beta.json`
- `https://downloads.example.com/cloudagent/releases/v0.1.45.json`

The metadata endpoint is the compatibility surface that should remain stable
even if the actual archive host changes.

### 3.3 Remote Archives

The archive host may be:

- GitHub Releases
- Aliyun OSS + CDN
- Tencent COS + CDN
- Qiniu Kodo + CDN
- another object storage or CDN service

Clients should treat archive URLs as metadata values, not as hard-coded
knowledge.

## 4. Version Rules

- All release tags use a leading `v`, for example `v0.1.44`
- Stable releases use plain semantic version tags, for example `v0.1.44`
- Pre-release builds use semantic version suffixes, for example:
  - `v0.1.44-beta.1`
  - `v0.1.44-alpha.1`
- Build metadata such as `+build.7` is allowed if needed

Release tags are validated by the shared tag helpers:

- `scripts/release_tag_rules.sh`
- `scripts/release-tag-rules.ps1`
- `scripts/validate-release-tag.ps1`

## 5. Release Artifacts

Each published release should include:

- platform archives for Linux, macOS, and Windows
- a checksum manifest
- release notes
- local installer payloads only if needed for first install or disaster recovery
- release metadata documents

Recommended archive naming:

- `cloudagent-vX.Y.Z-windows-x64.zip`
- `cloudagent-vX.Y.Z-linux-x64.tar.gz`
- `cloudagent-vX.Y.Z-linux-arm64.tar.gz`
- `cloudagent-vX.Y.Z-macos-x64.tar.gz`
- `cloudagent-vX.Y.Z-macos-arm64.tar.gz`

Recommended metadata files:

- `channels/stable.json`
- `channels/beta.json`
- `channels/alpha.json`
- `releases/vX.Y.Z.json`

## 6. Metadata Contract

Metadata should be intentionally small and versioned.

Recommended top-level shape:

```json
{
  "schema_version": 1,
  "channel": "stable",
  "version": "0.1.45",
  "tag": "v0.1.45",
  "published_at": "2026-06-27T12:00:00Z",
  "notes_url": "https://example.com/cloudagent/releases/v0.1.45",
  "assets": {
    "windows-x64": {
      "url": "https://downloads.example.com/cloudagent/v0.1.45/cloudagent-v0.1.45-windows-x64.zip",
      "sha256": "..."
    },
    "linux-x64": {
      "url": "https://downloads.example.com/cloudagent/v0.1.45/cloudagent-v0.1.45-linux-x64.tar.gz",
      "sha256": "..."
    },
    "linux-arm64": {
      "url": "https://downloads.example.com/cloudagent/v0.1.45/cloudagent-v0.1.45-linux-arm64.tar.gz",
      "sha256": "..."
    },
    "macos-x64": {
      "url": "https://downloads.example.com/cloudagent/v0.1.45/cloudagent-v0.1.45-macos-x64.tar.gz",
      "sha256": "..."
    },
    "macos-arm64": {
      "url": "https://downloads.example.com/cloudagent/v0.1.45/cloudagent-v0.1.45-macos-arm64.tar.gz",
      "sha256": "..."
    }
  }
}
```

Rules for the metadata contract:

- clients must reject unknown or unsupported `schema_version` values
- clients may ignore unknown optional fields
- archive URLs may change at any time
- the metadata endpoint itself should be treated as the durable client contract

## 7. Distribution Topology

CloudAgent should support multiple source tiers.

Recommended priority order:

1. user-configured explicit source
2. official CloudAgent mirror metadata endpoint
3. official CloudAgent GitHub fallback

This means the install and upgrade code should support distinct configuration
for:

- metadata base URL
- archive base URL, if not already embedded in metadata
- optional fallback metadata base URL

Recommended environment variables:

- `CLOUDAGENT_METADATA_BASE_URL`
- `CLOUDAGENT_METADATA_FALLBACK_URL`
- `CLOUDAGENT_RELEASE_CHANNEL`

Current migration note:

- today the default metadata still ships from GitHub Release assets
- clients should first try `<channel>.json`, such as `stable.json`, and then
  fall back to `latest.json` for backward compatibility
- this keeps the client contract stable while allowing the metadata host to
  move to a mirror or CDN later without requiring reinstall

The current script-source variables may remain during migration, but they
should be treated as transitional:

- `CLOUDAGENT_SCRIPT_BASE_URL`
- `CLOUDAGENT_SCRIPT_FALLBACK_URL`

## 8. Standard Release Flow

The standard release flow should be:

1. Bump the workspace version to the next release tag.
2. Commit the version bump with a clear release-oriented message.
3. Create a tag that matches the version, for example `v0.1.45`.
4. Push the tag to GitHub.
5. GitHub Actions triggers the release workflow from the tag push.
6. CI builds binaries for all supported platforms.
7. CI assembles platform archives.
8. CI generates checksums.
9. CI publishes the GitHub Release.
10. CI publishes metadata documents for the new release.
11. CI uploads archives and metadata to the official mirror or CDN.
12. CI optionally verifies mirror readability before marking the release done.

For prerelease builds, use tags such as:

- `v0.1.45-beta.1`
- `v0.1.45-alpha.1`

## 9. Multi-Platform Install Layout

All platforms should share the same logical structure:

- `releases/<version>/...`
- `current -> releases/<version>`
- a visible launcher path on the user's PATH

### 9.1 Windows

Recommended layout:

- install root: `%LOCALAPPDATA%\\CloudAgent`
- releases root: `%LOCALAPPDATA%\\CloudAgent\\releases`
- active pointer: `%LOCALAPPDATA%\\CloudAgent\\current`
- visible launcher dir: `%USERPROFILE%\\.local\\bin`

Implementation notes:

- `current` may be a junction
- visible launcher files may remain small local wrappers
- normal upgrade must not require downloading remote PowerShell helper scripts

### 9.2 Linux

Recommended layout:

- install root: `~/.local/share/cloudagent`
- releases root: `~/.local/share/cloudagent/releases`
- active pointer: `~/.local/share/cloudagent/current`
- visible launcher dir: `~/.local/bin`

Implementation notes:

- `current` should be a symlink
- visible launcher should point to the active local executable

### 9.3 macOS

Recommended layout:

- install root: `~/.local/share/cloudagent`
- releases root: `~/.local/share/cloudagent/releases`
- active pointer: `~/.local/share/cloudagent/current`
- visible launcher dir: `~/.local/bin`

Implementation notes:

- `current` should be a symlink
- architecture-specific archives must be selected correctly
- Rosetta-specific behavior should be explicit if ever supported

## 10. Install Flow

CloudAgent installation should be predictable and recoverable on every
platform.

The installer should:

1. Resolve the desired release channel and version.
2. Fetch release metadata from the configured metadata endpoint.
3. Select the correct platform asset from metadata.
4. Download the archive and verify its checksum.
5. Extract into a staged version directory.
6. Validate the extracted package layout.
7. Move the staged directory into `releases/<version>`.
8. Switch `current` only after extraction and validation succeed.
9. Refresh visible launchers and PATH entries after the version switch.

This keeps the active installation intact until the new version is ready.

## 11. Upgrade Flow

The upgrade flow should be platform-neutral.

The upgrader should:

1. Detect whether local managed processes are running.
2. Stop managed processes if replacement requires it.
3. Invoke the local installer logic for the target version.
4. Restart managed processes after a successful upgrade if they were running.

Critical rule:

- `cloudagent upgrade` must use local upgrade logic first
- network access is for metadata and archives only

If upgrade fails before the final pointer switch, the previous installed
version must remain usable.

## 12. Launcher Policy

Launchers exist to route execution to the local active install.

Launchers should:

- invoke the local current executable
- pass through user arguments
- remain stable across upgrades

Launchers should not be responsible for:

- downloading remote installer scripts during normal upgrades
- resolving GitHub API details
- embedding mirror-specific archive rules

Remote script entrypoints may still exist for:

- first install
- emergency repair
- manual recovery

But they are not the normal steady-state upgrade path.

## 13. Mirror Strategy

The official release process should be mirror-ready from the beginning.

Recommended policy:

- GitHub Release remains a valid source of truth
- official mirror metadata and assets are published for every release
- clients prefer the official metadata endpoint
- clients fall back to GitHub only when mirror resolution fails

This avoids a future migration where old clients can only speak to GitHub.

## 14. CI Responsibilities

CI should own cross-platform packaging and mirror publication.

The release workflow should:

- build Windows, Linux, and macOS binaries
- package each supported target
- compute and publish checksums
- generate release metadata documents
- upload archives to GitHub Release
- upload the same archives and metadata to the official mirror
- verify that metadata and archives are readable before completion

This means developers do not need local Linux or macOS environments to ship
multi-platform releases.

## 15. Migration Plan For Existing Users

Migration should be explicit and staged.

### Phase 1: Compatibility Bridge

Keep the current remote-script path only as a bridge for already-installed
clients.

That bridge should do the minimum required work:

- refresh the local launcher
- refresh or install the new local installer payload
- hand off future upgrades to local installer logic

### Phase 2: Local-First Upgrade

After the bridge release is broadly adopted:

- `cloudagent upgrade` should no longer require remote script execution
- metadata and archive retrieval become the only network dependency

### Phase 3: Mirror-Preferred Distribution

After metadata publishing is stable:

- official metadata endpoints become the default
- GitHub becomes fallback rather than the primary install contract

This avoids forcing users to uninstall and reinstall just to adopt a new
distribution topology.

## 16. Rollback Policy

CloudAgent should keep previous version directories whenever possible.

That gives two rollback paths:

- re-point `current` to a previous version directory
- reinstall a pinned older release using the local installer

There is no need to overwrite the last known-good install during normal
upgrades.

## 17. Beta And Prerelease Policy

Beta and alpha releases are first-class channels.

Recommended rules:

- use `vX.Y.Z-beta.N` for public beta testing
- use `vX.Y.Z-alpha.N` for earlier validation builds
- publish those tags as prereleases
- expose channel metadata separately from stable metadata
- never move stable users to prerelease builds implicitly

## 18. Uninstall Policy

CloudAgent should keep two uninstall paths:

### 18.1 Product Command

Users can uninstall through the installed product entrypoint:

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

### 18.2 Direct Bootstrap Entry

Users may also run a direct bootstrap uninstall entry for manual recovery.

That entry should be documented for all supported platforms, but it should be
treated as a recovery path, not the primary steady-state lifecycle path.

## 19. Recovery And Diagnostics

The installer and upgrader should emit enough detail to distinguish:

- metadata resolution failures
- archive download failures
- checksum failures
- extraction failures
- launcher refresh failures
- process restart failures

Recovery documentation should include:

- how to reinstall a pinned version
- how to point `current` back to a prior version
- how to override metadata endpoints
- how to force GitHub fallback if the mirror is down

## 20. Documentation Sources

The main implementation references are:

- `scripts/install.sh`
- `scripts/install.ps1`
- `scripts/upgrade.sh`
- `scripts/upgrade.ps1`
- `scripts/uninstall.sh`
- `scripts/uninstall.ps1`
- `scripts/stage-release-scripts.sh`
- `.github/workflows/release.yml`

When implementation diverges from this document, the implementation should be
treated as transitional and brought back into alignment with this design.
