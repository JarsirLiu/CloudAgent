#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
SCRIPT_BASE_URL="${CLOUDAGENT_SCRIPT_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
SCRIPT_FALLBACK_URL="${CLOUDAGENT_SCRIPT_FALLBACK_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
INSTALLS_DIR="$INSTALL_ROOT/installs"
CURRENT_LINK="$INSTALL_ROOT/current"
INSTALL_MARKER=".cloudagent-install-complete"
BIN_DIR="${CLOUDAGENT_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${CLOUDAGENT_DATA_DIR:-$HOME/.cloudagent}"
TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/cloudagent-install-$$"
VERSION="latest"
FORCE=0
STAGE_TOTAL=8
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

trap 'rm -rf "$WORK"' EXIT INT TERM

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
    curl --fail --location --progress-bar -H "User-Agent: cloudagent-installer" "$url" -o "$output"
  else
    curl -fsSL -H "User-Agent: cloudagent-installer" "$url" -o "$output"
  fi
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

resolve_latest_release_tag() {
  latest_tag=$(curl -fsSL -o /dev/null -w '%{url_effective}' "https://github.com/$REPO/releases/latest" | awk -F/ '{print $NF}')
  if is_semver_tag "$latest_tag"; then
    printf '%s\n' "$latest_tag"
    return 0
  fi

  echo "failed to resolve release version" >&2
  exit 1
}

fetch_release_metadata() {
  stage_start 1 "Resolving release metadata"
  if [ "$VERSION" = "latest" ]; then
    RELEASE_TAG=$(resolve_latest_release_tag)
  else
    RELEASE_TAG=$(normalize_release_tag "$VERSION")
  fi
  [ -n "$RELEASE_TAG" ] || {
    echo "failed to resolve release version" >&2
    exit 1
  }
  RELEASE_VERSION=${RELEASE_TAG#v}
  ASSET_BASENAME="cloudagent-${RELEASE_TAG}-${OS}-${ARCH}.tar.gz"
  ASSET_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/$ASSET_BASENAME"
  CHECKSUM_URL="https://github.com/$REPO/releases/download/$RELEASE_TAG/SHA256SUMS"
  stage_done "($RELEASE_TAG)"
}

current_version() {
  if [ -L "$CURRENT_LINK" ]; then
    basename "$(readlink "$CURRENT_LINK")"
  fi
}

verify_checksum() {
  asset="$1"
  checksum_file="$WORK/SHA256SUMS"
  stage_start 3 "Downloading checksum manifest"
  curl_download "$CHECKSUM_URL" "$checksum_file"
  checksum_size=$(file_size "$checksum_file")
  stage_done "($(human_size "$checksum_size"))"

  stage_start 4 "Verifying package checksum"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$WORK" && grep "  $ASSET_BASENAME\$" "$checksum_file" | sha256sum -c -)
  elif command -v shasum >/dev/null 2>&1; then
    expected=$(grep "  $ASSET_BASENAME\$" "$checksum_file" | awk '{print $1}')
    actual=$(shasum -a 256 "$asset" | awk '{print $1}')
    [ "$expected" = "$actual" ]
  else
    echo "warning: no sha256 tool found; skipping checksum verification" >&2
    stage_done "(skipped)"
    return 0
  fi
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
  stage_start 5 "Extracting package"
  tar -xzf "$asset" -C "$unpack_root"
  package_dir=$(find "$unpack_root" -mindepth 1 -maxdepth 1 -type d | head -n 1 || true)
  if [ -z "$package_dir" ]; then
    echo "invalid archive layout: missing package directory" >&2
    exit 1
  fi
  STAGED_DIR="$package_dir"
  stage_done
}

install_files() {
  target="$INSTALLS_DIR/$RELEASE_VERSION"
  if [ "$FORCE" -ne 1 ] && [ "$(current_version || true)" = "$RELEASE_VERSION" ] && [ -f "$target/$INSTALL_MARKER" ]; then
    printf 'CloudAgent %s is already installed\n' "$RELEASE_VERSION" >&2
    return 0
  fi
  mkdir -p "$INSTALLS_DIR" "$BIN_DIR" "$DATA_DIR"
  if [ -e "$target" ]; then
    printf 'Replacing existing installation at %s\n' "$target" >&2
    rm -rf "$target"
  fi
  stage_start 6 "Installing files"
  mkdir -p "$target"
  cp -R "$STAGED_DIR"/. "$target"/
  printf 'Updating current launcher target\n' >&2
  ln -sfn "$target" "$CURRENT_LINK"
  stage_done
}

write_launchers() {
  stage_start 7 "Refreshing command launchers"
  cat > "$BIN_DIR/cloudagent" <<EOF
#!/usr/bin/env sh
set -eu

SCRIPT_BASE_URL="$SCRIPT_BASE_URL"
SCRIPT_FALLBACK_URL="$SCRIPT_FALLBACK_URL"

run_remote_script() {
  script_url="\$1"
  shift
  tmp_script="\$(mktemp "\${TMPDIR:-/tmp}/cloudagent-remote-XXXXXX")"
  trap 'rm -f "\$tmp_script"' EXIT INT TERM
  for base_url in "\$SCRIPT_BASE_URL" "\$SCRIPT_FALLBACK_URL"; do
    [ -n "\$base_url" ] || continue
    if curl -fsSL "\${base_url%/}/\$script_url" -o "\$tmp_script"; then
      sh "\$tmp_script" "\$@"
      return 0
    fi
    rm -f "\$tmp_script"
  done

  echo "failed to download \$script_url from configured script sources" >&2
  return 1
}

case "\${1:-}" in
  upgrade)
    shift
    run_remote_script "upgrade.sh" "\$@"
    ;;
  uninstall)
    shift
    run_remote_script "uninstall.sh" "\$@"
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
  : > "$target/$INSTALL_MARKER"
  stage_done
}

need_cmd curl
need_cmd tar
detect_os
detect_arch
fetch_release_metadata
download_and_unpack
install_files
write_launchers

printf 'CloudAgent %s installed\n' "$RELEASE_VERSION"
printf 'install root: %s\n' "$INSTALL_ROOT"
printf 'data dir: %s\n' "$DATA_DIR"
printf 'bin dir: %s\n' "$BIN_DIR"
printf 'run: %s/cloudagent start\n' "$BIN_DIR"
