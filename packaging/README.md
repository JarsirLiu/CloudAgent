# Packaging Assets

This directory is the single home for release-packaging resources.

- `linux/` or top-level Linux packaging manifests such as `nfpm.yaml`
- `windows/wix/` for Windows MSI assets used by WiX / `cargo wix`

Keep installer templates, license sidecars, icons, and other distribution-only
files here instead of scattering them under app or crate source directories.

Common entrypoints:

- Linux packages: `nfpm pkg --config packaging/nfpm.yaml ...`
- Windows MSI: `pwsh ./scripts/package-windows.ps1 -Target x86_64-pc-windows-msvc -Version 0.1.34`
