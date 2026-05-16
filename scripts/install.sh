#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
INSTALLS_DIR="$INSTALL_ROOT/installs"
CURRENT_LINK="$INSTALL_ROOT/current"
BIN_DIR="${CLOUDAGENT_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${CLOUDAGENT_DATA_DIR:-$HOME/.cloudagent}"
TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/cloudagent-install-$$"
VERSION="latest"
FORCE=0

trap 'rm -rf "$WORK"' EXIT INT TERM

curl_download() {
  url="$1"
  output="$2"
  label="$3"

  echo "$label"
  mkdir -p "$(dirname "$output")"
  if [ -t 2 ]; then
    curl --fail --location --progress-bar -H "User-Agent: cloudagent-installer" "$url" -o "$output"
  else
    curl -fsSL -H "User-Agent: cloudagent-installer" "$url" -o "$output"
  fi
}

usage() {
  cat <<'EOF'
CloudAgent installer

Usage:
  install.sh [--version VERSION] [--force]

Options:
  --version VERSION  Install a specific release version (for example 0.1.7).
                     Defaults to the latest GitHub release.
  --force            Reinstall even if the target version is already current.
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

release_api_url() {
  if [ "$VERSION" = "latest" ]; then
    printf '%s' "https://api.github.com/repos/$REPO/releases/latest"
  else
    printf '%s' "https://api.github.com/repos/$REPO/releases/tags/v$VERSION"
  fi
}

extract_field() {
  key="$1"
  sed -n "s/.*\"$key\"[[:space:]]*:[[:space:]]*\"\([^\"]*\)\".*/\1/p"
}

fetch_release_metadata() {
  mkdir -p "$WORK"
  metadata="$WORK/release.json"
  echo "Resolving release metadata"
  curl_download "$(release_api_url)" "$metadata" "Fetching release metadata"
  RELEASE_JSON="$metadata"
  RELEASE_TAG=$(grep -m1 '"tag_name"' "$metadata" | extract_field tag_name)
  if [ -z "$RELEASE_TAG" ]; then
    echo "failed to resolve release version" >&2
    exit 1
  fi
  RELEASE_VERSION=${RELEASE_TAG#v}
  ASSET_BASENAME="cloudagent-${RELEASE_TAG}-${OS}-${ARCH}.tar.gz"
  ASSET_URL=$(sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$metadata" | grep "/$ASSET_BASENAME\$" | head -n 1 || true)
  CHECKSUM_URL=$(sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' "$metadata" | grep '/SHA256SUMS$' | head -n 1 || true)
  if [ -z "$ASSET_URL" ]; then
    echo "could not find asset $ASSET_BASENAME in release $RELEASE_TAG" >&2
    exit 1
  fi
}

current_version() {
  if [ -L "$CURRENT_LINK" ]; then
    basename "$(readlink "$CURRENT_LINK")"
  fi
}

verify_checksum() {
  asset="$1"
  if [ -z "$CHECKSUM_URL" ]; then
    echo "warning: no SHA256SUMS asset found; skipping checksum verification" >&2
    return 0
  fi
  checksum_file="$WORK/SHA256SUMS"
  curl_download "$CHECKSUM_URL" "$checksum_file" "Downloading checksum manifest"
  echo "Verifying package checksum"
  if command -v sha256sum >/dev/null 2>&1; then
    (cd "$WORK" && grep "  $ASSET_BASENAME\$" "$checksum_file" | sha256sum -c -)
  elif command -v shasum >/dev/null 2>&1; then
    expected=$(grep "  $ASSET_BASENAME\$" "$checksum_file" | awk '{print $1}')
    actual=$(shasum -a 256 "$asset" | awk '{print $1}')
    [ "$expected" = "$actual" ]
  else
    echo "warning: no sha256 tool found; skipping checksum verification" >&2
  fi
}

download_and_unpack() {
  asset="$WORK/$ASSET_BASENAME"
  curl_download "$ASSET_URL" "$asset" "Downloading CloudAgent $RELEASE_VERSION"
  verify_checksum "$asset"
  unpack_root="$WORK/unpack"
  mkdir -p "$unpack_root"
  echo "Extracting package"
  tar -xzf "$asset" -C "$unpack_root"
  package_dir=$(find "$unpack_root" -mindepth 1 -maxdepth 1 -type d | head -n 1 || true)
  if [ -z "$package_dir" ]; then
    echo "invalid archive layout: missing package directory" >&2
    exit 1
  fi
  STAGED_DIR="$package_dir"
}

install_files() {
  target="$INSTALLS_DIR/$RELEASE_VERSION"
  if [ "$FORCE" -ne 1 ] && [ "$(current_version || true)" = "$RELEASE_VERSION" ] && [ -d "$target" ]; then
    echo "cloudagent $RELEASE_VERSION is already installed"
    return 0
  fi
  mkdir -p "$INSTALLS_DIR" "$BIN_DIR" "$DATA_DIR"
  if [ -e "$target" ]; then
    echo "Replacing existing installation at $target"
    rm -rf "$target"
  fi
  echo "Installing files to $target"
  mkdir -p "$target"
  cp -R "$STAGED_DIR"/. "$target"/
  echo "Updating current launcher target"
  ln -sfn "$target" "$CURRENT_LINK"
}

write_launchers() {
  echo "Refreshing command launchers"
  cat > "$BIN_DIR/cloudagent" <<EOF
#!/usr/bin/env sh
set -eu

case "\${1:-}" in
  upgrade)
    shift
    exec curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/upgrade.sh | sh -s -- "\$@"
    ;;
  uninstall)
    shift
    exec curl -fsSL https://raw.githubusercontent.com/$REPO/main/scripts/uninstall.sh | sh -s -- "\$@"
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
}

ensure_path() {
  case ":$PATH:" in
    *":$BIN_DIR:"*) return 0 ;;
  esac

  path_line='export PATH="$HOME/.local/bin:$PATH"'
  touched=0
  for rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.zprofile"; do
    [ -f "$rc" ] || continue
    if ! grep -Fq "$path_line" "$rc"; then
      printf '\n# CloudAgent\n%s\n' "$path_line" >> "$rc"
      echo "updated PATH in $rc"
      touched=1
    fi
  done

  fish_config="$HOME/.config/fish/config.fish"
  if [ -f "$fish_config" ] && ! grep -Fq 'fish_add_path "$HOME/.local/bin"' "$fish_config"; then
    printf '\n# CloudAgent\nfish_add_path "$HOME/.local/bin"\n' >> "$fish_config"
    echo "updated PATH in $fish_config"
    touched=1
  fi

  if [ "$touched" -eq 0 ]; then
    echo "add $BIN_DIR to PATH to use cloudagent from new terminals" >&2
  fi
}

need_cmd curl
need_cmd tar
detect_os
detect_arch
fetch_release_metadata
download_and_unpack
install_files
write_launchers
ensure_path

echo "installed CloudAgent $RELEASE_VERSION"
echo "install root: $INSTALL_ROOT"
echo "data dir: $DATA_DIR"
echo "bin dir: $BIN_DIR"
echo "run: cloudagent start"
