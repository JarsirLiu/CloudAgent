#!/usr/bin/env sh
set -eu

INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
BIN_DIR="${CLOUDAGENT_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${CLOUDAGENT_DATA_DIR:-$HOME/.cloudagent}"
PURGE=0
STAGE_TOTAL=3

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

current_version() {
  current_link="$INSTALL_ROOT/current"
  if [ -L "$current_link" ]; then
    basename "$(readlink "$current_link")"
  fi
}

stage_start() {
  step="$1"
  title="$2"
  printf '[%s/%s] %s... ' "$step" "$STAGE_TOTAL" "$title"
}

stage_done() {
  detail="${1:-}"
  if [ -n "$detail" ]; then
    printf 'done %s\n' "$detail"
  else
    printf 'done\n'
  fi
}

printf '🧹 Uninstalling CloudAgent\n'
if version=$(current_version 2>/dev/null); then
  printf 'CloudAgent %s\n' "$version"
fi
printf '\n'

stage_start 1 "Removing launchers"
launcher_removed=0
for name in cloudagent cli node agentd; do
  target="$BIN_DIR/$name"
  if [ -L "$target" ] || [ -f "$target" ]; then
    rm -f "$target"
    launcher_removed=1
  fi
done
cleanup_path
if [ "$launcher_removed" -eq 1 ]; then
  stage_done
else
  stage_done "(already removed)"
fi

stage_start 2 "Removing installation"
if [ -d "$INSTALL_ROOT" ]; then
  rm -rf "$INSTALL_ROOT"
  stage_done
else
  stage_done "(already removed)"
fi

data_stage_title="Keeping user data"
if [ "$PURGE" -eq 1 ]; then
  data_stage_title="Removing user data"
fi

stage_start 3 "$data_stage_title"
if [ "$PURGE" -eq 1 ] && [ -d "$DATA_DIR" ]; then
  rm -rf "$DATA_DIR"
  stage_done
  printf 'CloudAgent removed\n'
  printf 'User data removed: %s\n' "$DATA_DIR"
else
  stage_done "(kept)"
  printf 'CloudAgent removed\n'
  printf 'User data kept: %s\n' "$DATA_DIR"
fi
