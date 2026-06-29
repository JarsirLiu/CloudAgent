#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
SCRIPT_BASE_URL="${CLOUDAGENT_SCRIPT_BASE_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
SCRIPT_FALLBACK_URL="${CLOUDAGENT_SCRIPT_FALLBACK_URL:-https://github.com/$REPO/releases/latest/download}"
METADATA_BASE_URL="${CLOUDAGENT_METADATA_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
METADATA_FALLBACK_URL="${CLOUDAGENT_METADATA_FALLBACK_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
CLOUDAGENT_RELEASE_CHANNEL="${CLOUDAGENT_RELEASE_CHANNEL:-stable}"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
INSTALLS_DIR="$INSTALL_ROOT/installs"
CURRENT_LINK="$INSTALL_ROOT/current"
INSTALL_MARKER=".cloudagent-install-complete"
SUPPORT_DIR_NAME="support"
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
  release_json="$(download_text "https://api.github.com/repos/$REPO/releases/latest")"
  latest_tag="$(printf '%s\n' "$release_json" | sed -n 's/.*"tag_name":[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
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
  release_tag="$(printf '%s\n' "$metadata_json" | sed -n 's/.*"tag"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  if [ -n "$release_tag" ]; then
    printf '%s\n' "$release_tag"
    return 0
  fi

  release_version="$(printf '%s\n' "$metadata_json" | sed -n 's/.*"version"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1)"
  if [ -n "$release_version" ]; then
    normalize_release_tag "$release_version"
    return 0
  fi

  printf '%s\n' "$metadata_json" | sed -n 's/.*"stable"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | head -n 1
}

metadata_asset_url() {
  metadata_json="$1"
  asset_key="$2"

  printf '%s\n' "$metadata_json" | awk -v asset_key="$asset_key" '
    BEGIN {
      in_asset = 0
      depth = 0
      asset_depth = 0
    }
    /"[^"]+"[[:space:]]*:[[:space:]]*\{/ {
      line = $0
      key = line
      sub(/^[[:space:]]*"/, "", key)
      sub(/".*$/, "", key)
      if (key == asset_key) {
        in_asset = 1
        asset_depth = depth + 1
      }
    }
    in_asset && /"url"[[:space:]]*:[[:space:]]*"[^"]+"/ {
      url = $0
      sub(/^.*"url"[[:space:]]*:[[:space:]]*"/, "", url)
      sub(/".*$/, "", url)
      print url
      exit
    }
    {
      line = $0
      opens = gsub(/\{/, "{", line)
      closes = gsub(/\}/, "}", line)
      depth += opens - closes
      if (in_asset && depth < asset_depth) {
        in_asset = 0
      }
    }
  '
}

metadata_asset_sha256() {
  metadata_json="$1"
  asset_key="$2"

  printf '%s\n' "$metadata_json" | awk -v asset_key="$asset_key" '
    BEGIN {
      in_asset = 0
      depth = 0
      asset_depth = 0
    }
    /"[^"]+"[[:space:]]*:[[:space:]]*\{/ {
      line = $0
      key = line
      sub(/^[[:space:]]*"/, "", key)
      sub(/".*$/, "", key)
      if (key == asset_key) {
        in_asset = 1
        asset_depth = depth + 1
      }
    }
    in_asset && /"sha256"[[:space:]]*:[[:space:]]*"[^"]+"/ {
      sha = $0
      sub(/^.*"sha256"[[:space:]]*:[[:space:]]*"/, "", sha)
      sub(/".*$/, "", sha)
      print sha
      exit
    }
    {
      line = $0
      opens = gsub(/\{/, "{", line)
      closes = gsub(/\}/, "}", line)
      depth += opens - closes
      if (in_asset && depth < asset_depth) {
        in_asset = 0
      }
    }
  '
}

release_asset_digest() {
  asset="$1"
  resolved_version="$2"
  release_json="$(download_text "https://api.github.com/repos/$REPO/releases/tags/$resolved_version")"

  digest="$(printf '%s\n' "$release_json" | awk -v asset="$asset" '
    /"name":[[:space:]]*"[^"]+"/ {
      name = $0
      sub(/^.*"name":[[:space:]]*"/, "", name)
      sub(/".*$/, "", name)
      if (name == asset) {
        in_asset = 1
        asset_depth = depth
      }
    }

    in_asset && /"digest":[[:space:]]*"[^"]+"/ {
      digest = $0
      sub(/^.*"digest":[[:space:]]*"/, "", digest)
      sub(/".*$/, "", digest)
    }

    {
      line = $0
      opens = gsub(/\{/, "{", line)
      closes = gsub(/\}/, "}", line)
      depth += opens - closes

      if (in_asset && depth < asset_depth) {
        in_asset = 0
      }
    }

    END {
      if (digest != "") {
        print digest
      }
    }
  ')"

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

verify_checksum() {
  asset="$1"
  expected="${ASSET_SHA256:-}"
  if [ -z "$expected" ]; then
    expected="$(release_asset_digest "$ASSET_BASENAME" "$RELEASE_TAG")"
  fi
  stage_start 4 "Verifying package checksum"
  if command -v sha256sum >/dev/null 2>&1; then
    actual=$(sha256sum "$asset" | awk '{print $1}')
  elif command -v shasum >/dev/null 2>&1; then
    actual=$(shasum -a 256 "$asset" | awk '{print $1}')
  else
    echo "warning: no sha256 tool found; skipping checksum verification" >&2
    stage_done "(skipped)"
    return 0
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
  TARGET_DIR="$INSTALLS_DIR/$RELEASE_VERSION"
  if [ "$FORCE" -ne 1 ] && [ "$(current_version || true)" = "$RELEASE_VERSION" ] && [ -f "$TARGET_DIR/$INSTALL_MARKER" ]; then
    printf 'CloudAgent %s is already installed\n' "$RELEASE_VERSION" >&2
    return 0
  fi
  mkdir -p "$INSTALLS_DIR" "$BIN_DIR" "$DATA_DIR"
  if [ -e "$TARGET_DIR" ]; then
    printf 'Replacing existing installation at %s\n' "$TARGET_DIR" >&2
    rm -rf "$TARGET_DIR"
  fi
  stage_start 6 "Installing files"
  mkdir -p "$TARGET_DIR"
  cp -R "$STAGED_DIR"/. "$TARGET_DIR"/
  support_dir="$TARGET_DIR/$SUPPORT_DIR_NAME"
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
  printf 'Updating current launcher target\n' >&2
  ln -sfn "$TARGET_DIR" "$CURRENT_LINK"
  stage_done
}

write_launchers() {
  stage_start 7 "Refreshing command launchers"
  cat > "$BIN_DIR/cloudagent" <<EOF
#!/usr/bin/env sh
set -eu

SCRIPT_BASE_URL="$SCRIPT_BASE_URL"
SCRIPT_FALLBACK_URL="$SCRIPT_FALLBACK_URL"
CURRENT_LINK="$CURRENT_LINK"
SUPPORT_DIR_NAME="$SUPPORT_DIR_NAME"

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

run_support_script() {
  script_name="\$1"
  shift
  local_script="\$CURRENT_LINK/\$SUPPORT_DIR_NAME/\$script_name"
  if [ -f "\$local_script" ]; then
    exec sh "\$local_script" "\$@"
  fi

  run_remote_script "\$script_name" "\$@"
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
  : > "$TARGET_DIR/$INSTALL_MARKER"
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
