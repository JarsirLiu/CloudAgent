#!/usr/bin/env sh
set -eu

INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
BIN_DIR="${CLOUDAGENT_BIN_DIR:-$HOME/.local/bin}"
DATA_DIR="${CLOUDAGENT_DATA_DIR:-$HOME/.cloudagent}"
PURGE=0
SELF_TEST=0
STAGE_TOTAL=3

usage() {
  cat <<'EOF'
CloudAgent uninstaller

Usage:
  uninstall.sh [--purge]
  uninstall.sh [--self-test]

Options:
  --purge    Also delete the user data directory (~/.cloudagent by default).
  --self-test Run cleanup self-tests and exit.
  -h, --help Show this help text.
EOF
}

while [ "$#" -gt 0 ]; do
  case "$1" in
    --purge)
      PURGE=1
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

run_self_test() {
  tmp_root="${TMPDIR:-/tmp}/cloudagent-uninstall-test-$$"
  rm -rf "$tmp_root"
  mkdir -p "$tmp_root/bin" "$tmp_root/home/.config/fish" "$tmp_root/install" "$tmp_root/data"

  old_home="${HOME-}"
  old_bin_dir="${CLOUDAGENT_BIN_DIR-}"
  old_install_root="${INSTALL_ROOT-}"
  old_data_dir="${DATA_DIR-}"

  HOME="$tmp_root/home"
  CLOUDAGENT_BIN_DIR="$tmp_root/bin"
  INSTALL_ROOT="$tmp_root/install"
  DATA_DIR="$tmp_root/data"

  cat > "$HOME/.profile" <<'EOF'
# CloudAgent
export PATH="$HOME/.local/bin:$PATH"
EOF
  cp "$HOME/.profile" "$HOME/.bashrc"
  cp "$HOME/.profile" "$HOME/.bash_profile"
  cp "$HOME/.profile" "$HOME/.zshrc"
  cp "$HOME/.profile" "$HOME/.zprofile"
  cat > "$HOME/.config/fish/config.fish" <<'EOF'
# CloudAgent
fish_add_path "$HOME/.local/bin"
EOF

  cat > "$CLOUDAGENT_BIN_DIR/cloudagent" <<'EOF'
#!/usr/bin/env sh
echo stub
EOF
  cat > "$CLOUDAGENT_BIN_DIR/cli" <<'EOF'
#!/usr/bin/env sh
echo stub
EOF
  cat > "$CLOUDAGENT_BIN_DIR/node" <<'EOF'
#!/usr/bin/env sh
echo stub
EOF
  cat > "$CLOUDAGENT_BIN_DIR/agentd" <<'EOF'
#!/usr/bin/env sh
echo stub
EOF
  chmod +x "$CLOUDAGENT_BIN_DIR/"*

  old_path="$PATH"
  cleanup_path
  [ -f "$HOME/.profile" ] || {
    echo "expected profile file to remain" >&2
    exit 1
  }
  [ -f "$CLOUDAGENT_BIN_DIR/cloudagent" ] || {
    echo "expected cloudagent launcher stub to remain" >&2
    exit 1
  }
  [ -f "$CLOUDAGENT_BIN_DIR/cli" ] || {
    echo "expected cli launcher stub to remain" >&2
    exit 1
  }
  [ -f "$CLOUDAGENT_BIN_DIR/node" ] || {
    echo "expected node launcher stub to remain" >&2
    exit 1
  }
  [ -f "$CLOUDAGENT_BIN_DIR/agentd" ] || {
    echo "expected agentd launcher stub to remain" >&2
    exit 1
  }
  if grep -q 'CloudAgent has been removed' "$CLOUDAGENT_BIN_DIR/cloudagent"; then
    echo "did not expect stub content in shell launcher" >&2
    exit 1
  fi

  PATH="$old_path"

  HOME="$old_home"
  CLOUDAGENT_BIN_DIR="$old_bin_dir"
  INSTALL_ROOT="$old_install_root"
  DATA_DIR="$old_data_dir"
  rm -rf "$tmp_root"
  echo "uninstall.sh self-test passed"
}

if [ "$SELF_TEST" -eq 1 ]; then
  run_self_test
  exit 0
fi

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
