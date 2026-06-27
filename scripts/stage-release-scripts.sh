#!/usr/bin/env sh
set -eu

REPO="${REPO:-${GITHUB_REPOSITORY:-JarsirLiu/CloudAgent}}"
SOURCE_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
SELF_TEST=0

if [ "${1:-}" = "--self-test" ]; then
  SELF_TEST=1
  shift
fi

if [ "$#" -gt 0 ]; then
  echo "unknown argument: $1" >&2
  exit 1
fi

require_env() {
  var_name="$1"
  eval "value=\${$var_name:-}"
  if [ -z "$value" ]; then
    echo "missing required environment variable: $var_name" >&2
    exit 1
  fi
}

copy_release_script() {
  src_name="$1"
  dest_dir="$2"
  cp "$SOURCE_DIR/$src_name" "$dest_dir/$src_name"
}

stage_release_scripts() {
  dest_dir="$1"
  channel="stable"
  published_at="${RELEASE_PUBLISHED_AT:-$(date -u +"%Y-%m-%dT%H:%M:%SZ")}"

  case "$RELEASE_TAG" in
    *-beta.*|*-beta)
      channel="beta"
      ;;
    *-alpha.*|*-alpha)
      channel="alpha"
      ;;
  esac

  mkdir -p "$dest_dir"
  copy_release_script install.sh "$dest_dir"
  copy_release_script upgrade.sh "$dest_dir"
  copy_release_script uninstall.sh "$dest_dir"
  copy_release_script release_tag_rules.sh "$dest_dir"
  copy_release_script install.ps1 "$dest_dir"
  copy_release_script upgrade.ps1 "$dest_dir"
  copy_release_script uninstall.ps1 "$dest_dir"
  copy_release_script release-tag-rules.ps1 "$dest_dir"
  copy_release_script validate-release-tag.ps1 "$dest_dir"

  printf '%s\n' "$RELEASE_TAG" > "$dest_dir/VERSION"
  for metadata_name in "latest.json" "${channel}.json"; do
    cat > "$dest_dir/$metadata_name" <<JSON
{
  "schema_version": 1,
  "channel": "${channel}",
  "version": "${RELEASE_TAG#v}",
  "tag": "${RELEASE_TAG}",
  "published_at": "${published_at}",
  "notes_url": "https://github.com/${REPO}/releases/tag/${RELEASE_TAG}",
  "stable": "${RELEASE_TAG}",
  "assets": {
    "linux-x64": {
      "url": "https://github.com/${REPO}/releases/download/${RELEASE_TAG}/cloudagent-${RELEASE_TAG}-linux-x64.tar.gz",
      "sha256": "${LINUX_X64_SHA}"
    },
    "macos-x64": {
      "url": "https://github.com/${REPO}/releases/download/${RELEASE_TAG}/cloudagent-${RELEASE_TAG}-macos-x64.tar.gz",
      "sha256": "${MACOS_X64_SHA}"
    },
    "macos-arm64": {
      "url": "https://github.com/${REPO}/releases/download/${RELEASE_TAG}/cloudagent-${RELEASE_TAG}-macos-arm64.tar.gz",
      "sha256": "${MACOS_ARM64_SHA}"
    },
    "windows-x64": {
      "url": "https://github.com/${REPO}/releases/download/${RELEASE_TAG}/cloudagent-${RELEASE_TAG}-windows-x64.zip",
      "sha256": "${WINDOWS_X64_SHA}"
    }
  }
}
JSON
  done
}

validate_release_scripts() {
  dest_dir="$1"

  for name in \
    install.sh \
    upgrade.sh \
    uninstall.sh \
    release_tag_rules.sh \
    install.ps1 \
    upgrade.ps1 \
    uninstall.ps1 \
    release-tag-rules.ps1 \
    validate-release-tag.ps1 \
    VERSION \
    latest.json
  do
    if [ ! -f "$dest_dir/$name" ]; then
      echo "missing release script file: $name" >&2
      exit 1
    fi
  done

  channel_name="stable"
  case "$RELEASE_TAG" in
    *-beta.*|*-beta)
      channel_name="beta"
      ;;
    *-alpha.*|*-alpha)
      channel_name="alpha"
      ;;
  esac

  if [ ! -f "$dest_dir/${channel_name}.json" ]; then
    echo "missing channel metadata file: ${channel_name}.json" >&2
    exit 1
  fi

  if [ "$(cat "$dest_dir/VERSION")" != "$RELEASE_TAG" ]; then
    echo "release VERSION does not match release tag" >&2
    exit 1
  fi

  if ! grep -F "\"stable\": \"${RELEASE_TAG}\"" "$dest_dir/latest.json" >/dev/null 2>&1; then
    echo "release latest.json does not include the release tag" >&2
    exit 1
  fi

  if ! grep -F "\"schema_version\": 1" "$dest_dir/latest.json" >/dev/null 2>&1; then
    echo "release latest.json does not include schema_version" >&2
    exit 1
  fi

  if ! grep -F "\"version\": \"${RELEASE_TAG#v}\"" "$dest_dir/latest.json" >/dev/null 2>&1; then
    echo "release latest.json does not include the release version" >&2
    exit 1
  fi

  if ! grep -F "cloudagent-${RELEASE_TAG}-windows-x64.zip" "$dest_dir/latest.json" >/dev/null 2>&1; then
    echo "release latest.json does not include release asset urls" >&2
    exit 1
  fi

  if ! grep -F "\"channel\": \"${channel_name}\"" "$dest_dir/${channel_name}.json" >/dev/null 2>&1; then
    echo "channel metadata file does not include the correct channel" >&2
    exit 1
  fi
}

if [ "$SELF_TEST" -eq 1 ]; then
  tmp_dir="$(mktemp -d)"
  trap 'rm -rf "$tmp_dir"' EXIT INT TERM

  RELEASE_TAG="v1.2.3"
  LINUX_X64_SHA="0000000000000000000000000000000000000000000000000000000000000000"
  MACOS_X64_SHA="1111111111111111111111111111111111111111111111111111111111111111"
  MACOS_ARM64_SHA="2222222222222222222222222222222222222222222222222222222222222222"
  WINDOWS_X64_SHA="3333333333333333333333333333333333333333333333333333333333333333"

  stage_release_scripts "$tmp_dir/release"
  validate_release_scripts "$tmp_dir/release"
  echo "stage-release-scripts.sh self-test passed"
  exit 0
fi

require_env RELEASE_TAG
require_env LINUX_X64_SHA
require_env MACOS_X64_SHA
require_env MACOS_ARM64_SHA
require_env WINDOWS_X64_SHA
require_env OUTPUT_DIR

stage_release_scripts "$OUTPUT_DIR"
validate_release_scripts "$OUTPUT_DIR"
