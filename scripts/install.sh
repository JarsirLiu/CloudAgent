#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
SCRIPT_BASE_URL="${CLOUDAGENT_SCRIPT_BASE_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
SCRIPT_FALLBACK_URL="${CLOUDAGENT_SCRIPT_FALLBACK_URL:-https://github.com/$REPO/releases/latest/download}"
METADATA_BASE_URL="${CLOUDAGENT_METADATA_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
METADATA_FALLBACK_URL="${CLOUDAGENT_METADATA_FALLBACK_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
CLOUDAGENT_RELEASE_CHANNEL="${CLOUDAGENT_RELEASE_CHANNEL:-stable}"
DEFAULT_INSTALL_ROOT="$HOME/.local/share/cloudagent"
LEGACY_INSTALL_ROOT="$HOME/.local/lib/cloudagent"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$DEFAULT_INSTALL_ROOT}"
RELEASES_DIR="$INSTALL_ROOT/releases"
CURRENT_LINK="$INSTALL_ROOT/current"
INSTALL_MARKER=".cloudagent-install-complete"
SUPPORT_DIR_NAME="support"
BIN_DIR="${CLOUDAGENT_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${CLOUDAGENT_DATA_DIR:-$HOME/.cloudagent}"
TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/cloudagent-install-$$"
VERSION="latest"
FORCE=0
STAGE_TOTAL=6
SELF_TEST=0

release_rules_path="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)/release_tag_rules.sh"
if [ -f "$release_rules_path" ]; then
  . "$release_rules_path"
else
  is_semver_tag() {
    case "$1" in
      v*)
        printf '%s\n' "$1" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
        ;;
      *)
        return 1
        ;;
    esac
  }

  normalize_release_tag() {
    version="$1"
    version="$(printf '%s' "$version" | tr -d '[:space:]')"
    case "$version" in
      '')
        echo "invalid release version: $1" >&2
        exit 1
        ;;
      v*)
        release_tag="$version"
        ;;
      *)
        release_tag="v$version"
        ;;
    esac

    if is_semver_tag "$release_tag"; then
      printf '%s\n' "$release_tag"
      return 0
    fi

    echo "invalid release version: $1" >&2
    exit 1
  }
fi

cleanup() {
  rm -rf "$WORK"
  if [ -n "${STAGED_TARGET_DIR:-}" ] && [ -e "$STAGED_TARGET_DIR" ]; then
    rm -rf "$STAGED_TARGET_DIR"
  fi
}

trap cleanup EXIT INT TERM

script_dir() {
  CDPATH= cd -- "$(dirname -- "$0")" && pwd
}

local_script_dir() {
  if [ -n "${0:-}" ] && [ -f "$0" ]; then
    script_dir
    return 0
  fi

  return 1
}

resolve_script_install_root() {
  local_dir="$(local_script_dir 2>/dev/null || true)"
  [ -n "$local_dir" ] || return 1

  case "$local_dir" in
    */current/support)
      dirname "$(dirname "$local_dir")"
      return 0
      ;;
    */releases/*/support)
      dirname "$(dirname "$(dirname "$local_dir")")"
      return 0
      ;;
  esac

  return 1
}

install_root_present() {
  root="$1"
  [ -n "$root" ] || return 1
  [ -L "$root/current" ] || [ -d "$root/releases" ] || [ -d "$root/installs" ]
}

resolve_install_root() {
  if [ -n "${CLOUDAGENT_INSTALL_ROOT:-}" ]; then
    printf '%s\n' "$CLOUDAGENT_INSTALL_ROOT"
    return 0
  fi

  script_root="$(resolve_script_install_root || true)"
  if [ -n "$script_root" ]; then
    printf '%s\n' "$script_root"
    return 0
  fi

  if install_root_present "$DEFAULT_INSTALL_ROOT"; then
    printf '%s\n' "$DEFAULT_INSTALL_ROOT"
    return 0
  fi

  if install_root_present "$LEGACY_INSTALL_ROOT"; then
    printf '%s\n' "$LEGACY_INSTALL_ROOT"
    return 0
  fi

  printf '%s\n' "$DEFAULT_INSTALL_ROOT"
}

support_script_names() {
  cat <<'EOF'
install.sh
upgrade.sh
uninstall.sh
release_tag_rules.sh
EOF
}

script_download_base_urls() {
  if [ -n "${RELEASE_TAG:-}" ]; then
    printf 'https://github.com/%s/releases/download/%s\n' "$REPO" "$RELEASE_TAG"
  fi
  printf '%s\n' "$SCRIPT_BASE_URL"
  printf '%s\n' "$SCRIPT_FALLBACK_URL"
}

stage_start() {
  step="$1"
  title="$2"
  printf '[%s/%s] %s... ' "$step" "$STAGE_TOTAL" "$title" >&2
}

stage_done() {
  detail="${1:-}"
  if [ -n "$detail" ]; then
    printf 'done %s\n' "$detail" >&2
  else
    printf 'done\n' >&2
  fi
}

human_size() {
  bytes="$1"
  awk -v b="$bytes" 'BEGIN {
    if (b >= 1024*1024*1024) {
      printf "%.1f GB", b / (1024*1024*1024)
    } else if (b >= 1024*1024) {
      printf "%.1f MB", b / (1024*1024)
    } else if (b >= 1024) {
      printf "%.1f KB", b / 1024
    } else {
      printf "%d B", b
    }
  }'
}

download_text() {
  url="$1"

  if command -v curl >/dev/null 2>&1; then
    curl -fsSL -H "User-Agent: cloudagent-installer" "$url"
    return
  fi

  if command -v wget >/dev/null 2>&1; then
    wget -q -O - --header="User-Agent: cloudagent-installer" "$url"
    return
  fi

  echo "curl or wget is required to fetch metadata." >&2
  exit 1
}

file_size() {
  path="$1"
  if size=$(wc -c < "$path" 2>/dev/null); then
    printf '%s\n' "$size"
    return 0
  fi
  stat -c%s "$path" 2>/dev/null || stat -f%z "$path"
}

curl_download() {
  url="$1"
  output="$2"
  mkdir -p "$(dirname "$output")"
  if [ -t 2 ]; then
    if curl --fail --location --progress-bar -H "User-Agent: cloudagent-installer" "$url" -o "$output"; then
      return 0
    fi
  else
    if curl -fsSL -H "User-Agent: cloudagent-installer" "$url" -o "$output"; then
      return 0
    fi
  fi

  if command -v wget >/dev/null 2>&1; then
    if [ -t 2 ]; then
      if wget --show-progress --header="User-Agent: cloudagent-installer" -O "$output" "$url"; then
        return 0
      fi
    else
      if wget -q -O "$output" --header="User-Agent: cloudagent-installer" "$url"; then
        return 0
      fi
    fi
  fi

  return 1
}

download_remote_script() {
  script_name="$1"
  output="$2"
  for base_url in "$SCRIPT_BASE_URL" "$SCRIPT_FALLBACK_URL"; do
    [ -n "$base_url" ] || continue
    if curl_download "${base_url%/}/$script_name" "$output"; then
      return 0
    fi
    rm -f "$output"
  done

  echo "failed to download $script_name from configured script sources" >&2
  exit 1
}

download_support_script() {
  script_name="$1"
  output="$2"
  for base_url in $(script_download_base_urls); do
    [ -n "$base_url" ] || continue
    if curl_download "${base_url%/}/$script_name" "$output"; then
      return 0
    fi
    rm -f "$output"
  done

  echo "failed to download $script_name from configured support sources" >&2
  exit 1
}

usage() {
  cat <<'EOF'
CloudAgent installer

Usage:
  install.sh [--version VERSION] [--force]
  install.sh [--self-test]

Options:
  --version VERSION  Install a specific release version (for example 0.1.7).
                     Defaults to the latest GitHub release.
  --force            Reinstall even if the target version is already current.
  --self-test        Run tag validation self-tests and exit.
  -h, --help         Show this help text.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --version)
      VERSION="${2:-}"
      shift 2
      ;;
    --force)
      FORCE=1
      shift
      ;;
    --self-test)
      SELF_TEST=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown option: $1" >&2
      usage >&2
      exit 1
      ;;
  esac
done

run_self_test() {
  valid_tags="v0.1.0 v1.2.3 v1.2.3-beta.1 v1.2.3+build.7 v1.2.3-beta.1+build.7"
  invalid_tags="v v1 v1.2 1.2.3 v01.2.3 v1.02.3 v1.2.03 v1.2.3- v1.2.3+"
  self_test_root="${TMPDIR:-/tmp}/cloudagent-install-root-test-$$"

  for tag in $valid_tags; do
    if ! is_semver_tag "$tag"; then
      echo "expected valid tag to pass: $tag" >&2
      exit 1
    fi
  done

  for tag in $invalid_tags; do
    if is_semver_tag "$tag"; then
      echo "expected invalid tag to fail: $tag" >&2
      exit 1
    fi
  done

  if [ "$(normalize_release_tag 1.2.3)" != "v1.2.3" ]; then
    echo "normalize_release_tag failed for 1.2.3" >&2
    exit 1
  fi

  if [ "$(normalize_release_tag v1.2.3-beta.1)" != "v1.2.3-beta.1" ]; then
    echo "normalize_release_tag failed for v1.2.3-beta.1" >&2
    exit 1
  fi

  if [ "$(resolve_install_root)" != "$DEFAULT_INSTALL_ROOT" ]; then
    echo "resolve_install_root should use the default install root when no override exists" >&2
    exit 1
  fi

  rm -rf "$self_test_root"
  mkdir -p "$self_test_root/default/releases" "$self_test_root/legacy/releases"
  DEFAULT_INSTALL_ROOT="$self_test_root/default"
  LEGACY_INSTALL_ROOT="$self_test_root/legacy"

  if [ "$(resolve_install_root)" != "$DEFAULT_INSTALL_ROOT" ]; then
    echo "resolve_install_root should prefer the default install root when both roots exist" >&2
    exit 1
  fi

  rm -rf "$DEFAULT_INSTALL_ROOT"
  if [ "$(resolve_install_root)" != "$LEGACY_INSTALL_ROOT" ]; then
    echo "resolve_install_root should fall back to the legacy install root when the default root is absent" >&2
    exit 1
  fi

  rm -rf "$self_test_root"
  DEFAULT_INSTALL_ROOT="$HOME/.local/share/cloudagent"
  LEGACY_INSTALL_ROOT="$HOME/.local/lib/cloudagent"

  sample_json='{"tag":"v1.2.3","version":"1.2.3","assets":{"linux-x64":{"url":"https://example.com/linux.tar.gz","sha256":"abc"}}}'
  if [ "$(json_read_field "$sample_json" tag)" != "v1.2.3" ]; then
    echo "json_read_field failed for tag" >&2
    exit 1
  fi
  if [ "$(json_read_field "$sample_json" assets linux-x64 url)" != "https://example.com/linux.tar.gz" ]; then
    echo "json_read_field failed for nested asset url" >&2
    exit 1
  fi

  release_json='{"assets":[{"name":"cloudagent-v1.2.3-linux-x64.tar.gz","digest":"sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"}]}'
  digest_value="$(json_find_asset_field_by_name "$release_json" "cloudagent-v1.2.3-linux-x64.tar.gz" digest)"
  if [ "$digest_value" != "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef" ]; then
    echo "json_find_asset_field_by_name failed for release asset digest" >&2
    exit 1
  fi

  echo "install.sh self-test passed"
}

if [ "$SELF_TEST" -eq 1 ]; then
  run_self_test
  exit 0
fi

need_cmd() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

detect_os() {
  case "$(uname -s)" in
    Linux*) OS="linux" ;;
    Darwin*) OS="macos" ;;
    *)
      echo "unsupported OS for install.sh; use GitHub Releases manually." >&2
      exit 1
      ;;
  esac
}

detect_arch() {
  case "$(uname -m)" in
    x86_64|amd64) ARCH="x64" ;;
    aarch64|arm64) ARCH="arm64" ;;
    *)
      echo "unsupported architecture: $(uname -m)" >&2
      exit 1
      ;;
  esac
}

json_read_field() {
  json_input="$1"
  shift

  if command -v python3 >/dev/null 2>&1; then
    printf '%s' "$json_input" | python3 -c '
import json
import sys

try:
    value = json.load(sys.stdin)
except Exception:
    sys.exit(1)

for segment in sys.argv[1:]:
    if isinstance(value, dict) and segment in value:
        value = value[segment]
    else:
        sys.exit(1)

if value is None or isinstance(value, (dict, list)):
    sys.exit(1)

sys.stdout.write(str(value))
' "$@"
    return $?
  fi

  if command -v python >/dev/null 2>&1; then
    printf '%s' "$json_input" | python -c '
import json
import sys

try:
    value = json.load(sys.stdin)
except Exception:
    sys.exit(1)

for segment in sys.argv[1:]:
    if isinstance(value, dict) and segment in value:
        value = value[segment]
    else:
        sys.exit(1)

if value is None or isinstance(value, (dict, list)):
    sys.exit(1)

sys.stdout.write(str(value))
' "$@"
    return $?
  fi

  if command -v perl >/dev/null 2>&1; then
    printf '%s' "$json_input" | perl -MJSON::PP -e '
use strict;
use warnings;

my $raw = do { local $/; <STDIN> };
my $value = eval { JSON::PP->new->decode($raw) };
exit 1 if $@;

for my $segment (@ARGV) {
  exit 1 unless ref($value) eq q(HASH) && exists $value->{$segment};
  $value = $value->{$segment};
}

exit 1 if !defined($value) || ref($value);
print $value;
' "$@"
    return $?
  fi

  if command -v node >/dev/null 2>&1; then
    printf '%s' "$json_input" | node -e '
const fs = require("fs");
let value;
try {
  value = JSON.parse(fs.readFileSync(0, "utf8"));
} catch {
  process.exit(1);
}
for (const segment of process.argv.slice(1)) {
  if (value && typeof value === "object" && !Array.isArray(value) && Object.prototype.hasOwnProperty.call(value, segment)) {
    value = value[segment];
  } else {
    process.exit(1);
  }
}
if (value === null || typeof value === "object") {
  process.exit(1);
}
process.stdout.write(String(value));
' "$@"
    return $?
  fi

  echo "missing required command: python3, python, perl, or node" >&2
  exit 1
}

json_find_asset_field_by_name() {
  json_input="$1"
  asset_name="$2"
  field_name="$3"

  if command -v python3 >/dev/null 2>&1; then
    printf '%s' "$json_input" | python3 -c '
import json
import sys

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)

for asset in data.get("assets", []):
    if isinstance(asset, dict) and asset.get("name") == sys.argv[1]:
        value = asset.get(sys.argv[2])
        if value is None or isinstance(value, (dict, list)):
            sys.exit(1)
        sys.stdout.write(str(value))
        sys.exit(0)

sys.exit(1)
' "$asset_name" "$field_name"
    return $?
  fi

  if command -v python >/dev/null 2>&1; then
    printf '%s' "$json_input" | python -c '
import json
import sys

try:
    data = json.load(sys.stdin)
except Exception:
    sys.exit(1)

for asset in data.get("assets", []):
    if isinstance(asset, dict) and asset.get("name") == sys.argv[1]:
        value = asset.get(sys.argv[2])
        if value is None or isinstance(value, (dict, list)):
            sys.exit(1)
        sys.stdout.write(str(value))
        sys.exit(0)

sys.exit(1)
' "$asset_name" "$field_name"
    return $?
  fi

  if command -v perl >/dev/null 2>&1; then
    printf '%s' "$json_input" | perl -MJSON::PP -e '
use strict;
use warnings;

my $raw = do { local $/; <STDIN> };
my $data = eval { JSON::PP->new->decode($raw) };
exit 1 if $@;
exit 1 unless ref($data) eq q(HASH) && ref($data->{assets}) eq q(ARRAY);

for my $asset (@{$data->{assets}}) {
  next unless ref($asset) eq q(HASH);
  next unless defined($asset->{name}) && $asset->{name} eq $ARGV[0];
  my $value = $asset->{$ARGV[1]};
  exit 1 if !defined($value) || ref($value);
  print $value;
  exit 0;
}

exit 1;
' "$asset_name" "$field_name"
    return $?
  fi

  if command -v node >/dev/null 2>&1; then
    printf '%s' "$json_input" | node -e '
const fs = require("fs");
let data;
try {
  data = JSON.parse(fs.readFileSync(0, "utf8"));
} catch {
  process.exit(1);
}
if (!data || !Array.isArray(data.assets)) {
  process.exit(1);
}
const asset = data.assets.find((entry) => entry && entry.name === process.argv[1]);
if (!asset) {
  process.exit(1);
}
const value = asset[process.argv[2]];
if (value === undefined || value === null || typeof value === "object") {
  process.exit(1);
}
process.stdout.write(String(value));
' "$asset_name" "$field_name"
    return $?
  fi

  echo "missing required command: python3, python, perl, or node" >&2
  exit 1
}

resolve_latest_release_tag() {
  release_json="$(download_text "https://api.github.com/repos/$REPO/releases/latest")"
  latest_tag="$(json_read_field "$release_json" tag_name 2>/dev/null || true)"
  if [ -n "$latest_tag" ] && is_semver_tag "$latest_tag"; then
    printf '%s\n' "$latest_tag"
    return 0
  fi

  echo "failed to resolve release version" >&2
  exit 1
}

metadata_url() {
  base_url="$1"
  metadata_name="$2"
  printf '%s/%s\n' "${base_url%/}" "$metadata_name"
}

fetch_latest_metadata() {
  channel_name="${CLOUDAGENT_RELEASE_CHANNEL:-stable}"
  for base_url in "$METADATA_BASE_URL" "$METADATA_FALLBACK_URL"; do
    [ -n "$base_url" ] || continue
    for metadata_name in "${channel_name}.json" "latest.json"; do
      if metadata_json="$(download_text "$(metadata_url "$base_url" "$metadata_name")" 2>/dev/null)"; then
        printf '%s\n' "$metadata_json"
        return 0
      fi
    done
  done

  return 1
}

metadata_release_tag() {
  metadata_json="$1"
  release_tag="$(json_read_field "$metadata_json" tag 2>/dev/null || true)"
  if [ -n "$release_tag" ]; then
    printf '%s\n' "$release_tag"
    return 0
  fi

  release_version="$(json_read_field "$metadata_json" version 2>/dev/null || true)"
  if [ -n "$release_version" ]; then
    normalize_release_tag "$release_version"
    return 0
  fi

  json_read_field "$metadata_json" stable 2>/dev/null || true
}

metadata_asset_url() {
  metadata_json="$1"
  asset_key="$2"
  json_read_field "$metadata_json" assets "$asset_key" url 2>/dev/null || true
}

metadata_asset_sha256() {
  metadata_json="$1"
  asset_key="$2"
  json_read_field "$metadata_json" assets "$asset_key" sha256 2>/dev/null || true
}

release_asset_digest() {
  asset="$1"
  resolved_version="$2"
  release_json="$(download_text "https://api.github.com/repos/$REPO/releases/tags/$resolved_version")"
  digest="$(json_find_asset_field_by_name "$release_json" "$asset" digest 2>/dev/null || true)"

  case "$digest" in
    sha256:????????????????????????????????????????????????????????????????)
      printf '%s\n' "${digest#sha256:}"
      ;;
    *)
      return 1
      ;;
  esac
}

release_asset_exists() {
  asset="$1"
  resolved_version="$2"
  release_asset_digest "$asset" "$resolved_version" >/dev/null 2>&1
}

release_url_for_asset() {
  asset="$1"
  resolved_version="$2"
  printf 'https://github.com/%s/releases/download/%s/%s\n' "$REPO" "$resolved_version" "$asset"
}

fetch_release_metadata() {
  stage_start 1 "Resolving release metadata"
  metadata_json=""
  if [ "$VERSION" = "latest" ]; then
    metadata_json="$(fetch_latest_metadata || true)"
    if [ -n "$metadata_json" ]; then
      RELEASE_TAG="$(metadata_release_tag "$metadata_json")"
    fi
    if [ -z "${RELEASE_TAG:-}" ]; then
      RELEASE_TAG=$(resolve_latest_release_tag)
    fi
  else
    RELEASE_TAG=$(normalize_release_tag "$VERSION")
  fi
  [ -n "$RELEASE_TAG" ] || {
    echo "failed to resolve release version" >&2
    exit 1
  }
  RELEASE_VERSION=${RELEASE_TAG#v}
  ASSET_KEY="${OS}-${ARCH}"
  ASSET_BASENAME="cloudagent-${RELEASE_TAG}-${OS}-${ARCH}.tar.gz"
  if [ -n "$metadata_json" ]; then
    ASSET_URL="$(metadata_asset_url "$metadata_json" "$ASSET_KEY")"
    ASSET_SHA256="$(metadata_asset_sha256 "$metadata_json" "$ASSET_KEY")"
  else
    ASSET_URL=""
    ASSET_SHA256=""
  fi
  if [ -z "$ASSET_URL" ]; then
    ASSET_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/$ASSET_BASENAME"
  fi
  stage_done "($RELEASE_TAG)"
}

current_version() {
  if [ -L "$CURRENT_LINK" ]; then
    basename "$(readlink "$CURRENT_LINK")"
  fi
}

current_link_target() {
  if [ -L "$CURRENT_LINK" ]; then
    readlink "$CURRENT_LINK" 2>/dev/null || true
  fi
}

already_installed_current_version() {
  target_dir="$RELEASES_DIR/$RELEASE_VERSION"
  current_target="$(current_link_target)"
  [ "$FORCE" -eq 0 ] &&
    [ -e "$target_dir/$INSTALL_MARKER" ] &&
    [ "$current_target" = "$target_dir" ]
}

verify_checksum() {
  asset="$1"
  expected="${ASSET_SHA256:-}"
  if [ -z "$expected" ]; then
    expected="$(release_asset_digest "$ASSET_BASENAME" "$RELEASE_TAG")"
  fi
  stage_start 3 "Verifying package checksum"
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$asset" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$asset" | awk '{print $1}')
  else
    echo "missing required command: sha256sum or shasum" >&2
    exit 1
  fi

  [ "$expected" = "$actual" ]
  stage_done
}

download_and_unpack() {
  asset="$WORK/$ASSET_BASENAME"
  stage_start 2 "Downloading release asset"
  curl_download "$ASSET_URL" "$asset"
  asset_size=$(file_size "$asset")
  stage_done "($(human_size "$asset_size"))"
  verify_checksum "$asset"
  unpack_root="$WORK/unpack"
  mkdir -p "$unpack_root"
  stage_start 4 "Extracting package"
  tar -xzf "$asset" -C "$unpack_root"
  package_dir=$(find "$unpack_root" -mindepth 1 -maxdepth 1 -type d | head -n 1 || true)
  if [ -z "$package_dir" ]; then
    echo "invalid archive layout: missing package directory" >&2
    exit 1
  fi
  for required_file in cloudagent cli node agentd; do
    if [ ! -e "$package_dir/$required_file" ]; then
      echo "invalid archive layout: missing $required_file" >&2
      exit 1
    fi
  done
  STAGED_DIR="$package_dir"
  stage_done
}

prepare_staged_release() {
  TARGET_DIR="$RELEASES_DIR/$RELEASE_VERSION"
  STAGED_TARGET_DIR="$INSTALL_ROOT/.staging/release-$RELEASE_VERSION-$$"
  stage_start 5 "Installing files"
  mkdir -p "$RELEASES_DIR" "$BIN_DIR" "$DATA_DIR" "$INSTALL_ROOT/.staging"
  rm -rf "$STAGED_TARGET_DIR"
  mkdir -p "$STAGED_TARGET_DIR"
  cp -R "$STAGED_DIR"/. "$STAGED_TARGET_DIR"/
  support_dir="$STAGED_TARGET_DIR/$SUPPORT_DIR_NAME"
  mkdir -p "$support_dir"
  local_dir=""
  if local_dir="$(local_script_dir 2>/dev/null)"; then
    have_all_local_support=1
    for file_name in $(support_script_names); do
      if [ ! -f "$local_dir/$file_name" ]; then
        have_all_local_support=0
        break
      fi
    done
  else
    have_all_local_support=0
  fi

  for file_name in $(support_script_names); do
    if [ "$have_all_local_support" -eq 1 ]; then
      cp "$local_dir/$file_name" "$support_dir/$file_name"
    else
      download_support_script "$file_name" "$support_dir/$file_name"
    fi
  done
  chmod 755 "$support_dir/install.sh" "$support_dir/upgrade.sh" "$support_dir/uninstall.sh"
  : > "$STAGED_TARGET_DIR/$INSTALL_MARKER"
  stage_done
}

activate_release() {
  current_target="$(current_link_target)"
  ALREADY_INSTALLED=0
  if [ "$FORCE" -eq 0 ] && [ -e "$TARGET_DIR/$INSTALL_MARKER" ] && [ "$current_target" = "$TARGET_DIR" ]; then
    printf 'CloudAgent %s is already installed\n' "$RELEASE_VERSION"
    ALREADY_INSTALLED=1
    STAGED_TARGET_DIR=""
    return 0
  fi

  if [ "$current_target" = "$TARGET_DIR" ] && [ -e "$TARGET_DIR" ]; then
    echo "refusing to replace the active version in place; use a different version or uninstall first" >&2
    exit 1
  fi

  BACKUP_TARGET_DIR=""
  if [ -e "$TARGET_DIR" ]; then
    BACKUP_TARGET_DIR="$INSTALL_ROOT/.staging/backup-$RELEASE_VERSION-$$"
    rm -rf "$BACKUP_TARGET_DIR"
    printf 'Replacing existing installation at %s\n' "$TARGET_DIR" >&2
    mv "$TARGET_DIR" "$BACKUP_TARGET_DIR"
  fi

  if mv "$STAGED_TARGET_DIR" "$TARGET_DIR"; then
    STAGED_TARGET_DIR=""
    if [ -n "$BACKUP_TARGET_DIR" ] && [ -e "$BACKUP_TARGET_DIR" ]; then
      rm -rf "$BACKUP_TARGET_DIR"
    fi
    return 0
  fi

  if [ -n "$BACKUP_TARGET_DIR" ] && [ -e "$BACKUP_TARGET_DIR" ] && [ ! -e "$TARGET_DIR" ]; then
    mv "$BACKUP_TARGET_DIR" "$TARGET_DIR" || true
  fi
  echo "failed to move the staged release into place" >&2
  exit 1
}

refresh_launchers_and_current() {
  stage_start 6 "Refreshing command launchers"
  current_tmp="$INSTALL_ROOT/.current-tmp-$$"
  current_backup=""
  rm -f "$current_tmp"
  if [ "$ALREADY_INSTALLED" -eq 0 ]; then
    ln -s "$TARGET_DIR" "$current_tmp"
    printf 'Updating current launcher target\n' >&2
    if [ -e "$CURRENT_LINK" ] || [ -L "$CURRENT_LINK" ]; then
      if [ ! -L "$CURRENT_LINK" ]; then
        rm -f "$current_tmp"
        echo "unexpected current path is not a symlink" >&2
        exit 1
      fi
      current_backup="$INSTALL_ROOT/.current-backup-$$"
      rm -rf "$current_backup"
      mv "$CURRENT_LINK" "$current_backup"
    fi
    if mv "$current_tmp" "$CURRENT_LINK"; then
      if [ -n "$current_backup" ] && { [ -e "$current_backup" ] || [ -L "$current_backup" ]; }; then
        rm -rf "$current_backup"
      fi
    else
      if [ -n "$current_backup" ] && { [ -e "$current_backup" ] || [ -L "$current_backup" ]; } && [ ! -e "$CURRENT_LINK" ] && [ ! -L "$CURRENT_LINK" ]; then
        mv "$current_backup" "$CURRENT_LINK" || true
      fi
      rm -f "$current_tmp"
      echo "failed to update current launcher target" >&2
      exit 1
    fi
  fi
  cat > "$BIN_DIR/cloudagent" <<EOF
#!/usr/bin/env sh
set -eu

CURRENT_LINK="$CURRENT_LINK"
SUPPORT_DIR_NAME="$SUPPORT_DIR_NAME"

run_support_script() {
  script_name="\$1"
  shift
  local_script="\$CURRENT_LINK/\$SUPPORT_DIR_NAME/\$script_name"
  if [ -f "\$local_script" ]; then
    exec sh "\$local_script" "\$@"
  fi

  echo "missing local support script: \$local_script" >&2
  echo "run the bootstrap installer again to repair this installation" >&2
  exit 1
}

case "\${1:-}" in
  upgrade)
    shift
    run_support_script "upgrade.sh" "\$@"
    ;;
  uninstall)
    shift
    run_support_script "uninstall.sh" "\$@"
    ;;
  *)
    exec "$CURRENT_LINK/cloudagent" "\$@"
    ;;
esac
EOF
  chmod 755 "$BIN_DIR/cloudagent"

  for name in cli node agentd; do
    cat > "$BIN_DIR/$name" <<EOF
#!/usr/bin/env sh
exec "$CURRENT_LINK/$name" "\$@"
EOF
    chmod 755 "$BIN_DIR/$name"
  done
  stage_done
}

need_cmd curl
need_cmd tar
detect_os
detect_arch
INSTALL_ROOT="$(resolve_install_root)"
RELEASES_DIR="$INSTALL_ROOT/releases"
CURRENT_LINK="$INSTALL_ROOT/current"
fetch_release_metadata
TARGET_DIR="$RELEASES_DIR/$RELEASE_VERSION"
if already_installed_current_version; then
  ALREADY_INSTALLED=1
  refresh_launchers_and_current
  printf 'CloudAgent %s is already installed\n' "$RELEASE_VERSION"
  printf 'install root: %s\n' "$INSTALL_ROOT"
  printf 'data dir: %s\n' "$DATA_DIR"
  printf 'bin dir: %s\n' "$BIN_DIR"
  printf 'run: %s/cloudagent start\n' "$BIN_DIR"
  exit 0
fi
download_and_unpack
prepare_staged_release
activate_release
refresh_launchers_and_current

printf 'CloudAgent %s installed\n' "$RELEASE_VERSION"
printf 'install root: %s\n' "$INSTALL_ROOT"
printf 'data dir: %s\n' "$DATA_DIR"
printf 'bin dir: %s\n' "$BIN_DIR"
printf 'run: %s/cloudagent start\n' "$BIN_DIR"
