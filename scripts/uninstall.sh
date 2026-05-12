#!/usr/bin/env sh
set -eu

INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
BIN_DIR="${CLOUDAGENT_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${CLOUDAGENT_DATA_DIR:-$HOME/.cloudagent}"
PURGE=0

usage() {
  cat <<'EOF'
CloudAgent uninstaller

Usage:
  uninstall.sh [--purge]

Options:
  --purge    Also delete the user data directory (~/.cloudagent by default).
  -h, --help Show this help text.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --purge)
      PURGE=1
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

cleanup_path() {
  path_line='export PATH="$HOME/.local/bin:$PATH"'
  fish_line='fish_add_path "$HOME/.local/bin"'

  rewrite_rc() {
    rc="$1"
    [ -f "$rc" ] || return 0
    tmp="$rc.cloudagent-tmp"
    awk -v path_line="$path_line" '
      $0 == "# CloudAgent" {
        if (getline next_line) {
          if (next_line == path_line) {
            changed = 1
            next
          }
          print $0
          print next_line
          next
        }
      }
      $0 == path_line {
        changed = 1
        next
      }
      { print }
      END {
        if (changed) {
          exit 0
        }
      }
    ' "$rc" > "$tmp" && mv "$tmp" "$rc"
    rm -f "$tmp"
  }

  for rc in "$HOME/.profile" "$HOME/.bashrc" "$HOME/.bash_profile" "$HOME/.zshrc" "$HOME/.zprofile"; do
    rewrite_rc "$rc"
  done

  fish_config="$HOME/.config/fish/config.fish"
  if [ -f "$fish_config" ]; then
    tmp="$fish_config.cloudagent-tmp"
    awk -v fish_line="$fish_line" '
      $0 == "# CloudAgent" {
        if (getline next_line) {
          if (next_line == fish_line) {
            changed = 1
            next
          }
          print $0
          print next_line
          next
        }
      }
      $0 == fish_line {
        changed = 1
        next
      }
      { print }
      END {
        if (changed) {
          exit 0
        }
      }
    ' "$fish_config" > "$tmp" && mv "$tmp" "$fish_config"
    rm -f "$tmp"
  fi
}

for name in cloudagent cli node agentd; do
  target="$BIN_DIR/$name"
  if [ -L "$target" ] || [ -f "$target" ]; then
    rm -f "$target"
    echo "removed: $target"
  fi
done

if [ -d "$INSTALL_ROOT" ]; then
  rm -rf "$INSTALL_ROOT"
  echo "removed: $INSTALL_ROOT"
fi

if [ "$PURGE" -eq 1 ] && [ -d "$DATA_DIR" ]; then
  rm -rf "$DATA_DIR"
  echo "removed: $DATA_DIR"
else
  echo "kept user data: $DATA_DIR"
fi

cleanup_path
