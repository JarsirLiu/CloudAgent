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
