$ErrorActionPreference = "Stop"

Write-Host "Running cargo fmt --all -- --check"
cargo fmt --all -- --check

Write-Host "Running cargo check --workspace --all-targets"
cargo check --workspace --all-targets

Write-Host "Running cargo clippy --workspace --all-targets -- -D warnings"
cargo clippy --workspace --all-targets -- -D warnings

Write-Host "Running cargo test --workspace --all-targets --no-fail-fast"
cargo test --workspace --all-targets --no-fail-fast
