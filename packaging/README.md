# Packaging Assets

This directory stores release-only assets that are not part of the runtime code.

Current release strategy:

- Linux and macOS publish `.tar.gz` archives
- Windows publishes `.zip` archives
- Installer scripts under `scripts/` download those archives and install them
- End-user install and upgrade commands are documented in `release-installation.md`

CloudAgent no longer ships platform installer packages such as MSI, PKG, DEB,
or RPM. Release packaging is intentionally archive-first so installation stays
scriptable and consistent across platforms.
