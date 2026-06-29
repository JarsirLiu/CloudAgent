#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
SCRIPT_BASE_URL="${CLOUDAGENT_SCRIPT_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
SCRIPT_FALLBACK_URL="${CLOUDAGENT_SCRIPT_FALLBACK_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
DEFAULT_INSTALL_ROOT="$HOME/.local/share/cloudagent"
LEGACY_INSTALL_ROOT="$HOME/.local/lib/cloudagent"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$DEFAULT_INSTALL_ROOT}"
CURRENT_LINK="$INSTALL_ROOT/current"
SUPPORT_DIR="$CURRENT_LINK/support"
CURRENT_EXE="$CURRENT_LINK/cloudagent"
CURRENT_NODE="$CURRENT_LINK/node"
CURRENT_AGENTD="$CURRENT_LINK/agentd"
TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/cloudagent-upgrade-$$"
STAGE_TOTAL=3

cleanup() {
  rm -rf "$WORK"
}

trap cleanup EXIT INT TERM

resolve_install_root() {
  if [ -n "${CLOUDAGENT_INSTALL_ROOT:-}" ]; then
    printf '%s\n' "$CLOUDAGENT_INSTALL_ROOT"
    return 0
  fi

  if [ -n "${0:-}" ] && [ -f "$0" ]; then
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    case "$script_dir" in
      */current/support)
        root_dir=$(dirname "$(dirname "$script_dir")")
        ;;
      */releases/*/support)
        root_dir=$(dirname "$(dirname "$(dirname "$script_dir")")")
        ;;
      *)
        root_dir=""
        ;;
    esac

    if [ -n "$root_dir" ]; then
      printf '%s\n' "$root_dir"
      return 0
    fi
  fi

  if [ -e "$LEGACY_INSTALL_ROOT/current" ] || [ -L "$LEGACY_INSTALL_ROOT/current" ]; then
    printf '%s\n' "$LEGACY_INSTALL_ROOT"
    return 0
  fi

  printf '%s\n' "$DEFAULT_INSTALL_ROOT"
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

node_running() {
  [ -x "$CURRENT_NODE" ] || return 1
  if command -v pgrep >/dev/null 2>&1; then
    pgrep -f "$CURRENT_NODE" >/dev/null 2>&1
    return $?
  fi
  ps -ef 2>/dev/null | grep -F "$CURRENT_NODE" | grep -v grep >/dev/null 2>&1
}

test_upgrade_restart_needed() {
  [ -x "$CURRENT_NODE" ] && node_running
}

stop_node_if_running() {
  [ -x "$CURRENT_NODE" ] || return 1

  stage_start 1 "Stopping local node"
  if command -v pkill >/dev/null 2>&1; then
    pkill -f "$CURRENT_AGENTD" >/dev/null 2>&1 || true
    pkill -f "$CURRENT_NODE" >/dev/null 2>&1 || true
  else
    agentd_pids="$(ps -ef 2>/dev/null | grep -F "$CURRENT_AGENTD" | grep -v grep | awk '{print $2}')"
    node_pids="$(ps -ef 2>/dev/null | grep -F "$CURRENT_NODE" | grep -v grep | awk '{print $2}')"
    for pid in $agentd_pids $node_pids; do
      [ -n "$pid" ] || continue
      kill "$pid" >/dev/null 2>&1 || true
    done
  fi
  stage_done "(stopped)"
  return 0
}

preflight_local_installer() {
  if [ -f "$SUPPORT_DIR/install.sh" ]; then
    return 0
  fi

  if [ -n "${0:-}" ] && [ -f "$0" ]; then
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    if [ -f "$script_dir/install.sh" ]; then
      return 0
    fi
  fi

  echo "missing local installer support at $SUPPORT_DIR/install.sh" >&2
  echo "run the bootstrap installer again to repair this installation" >&2
  exit 1
}

start_node_after_upgrade() {
  [ -x "$CURRENT_EXE" ] || {
    echo "upgrade completed but $CURRENT_EXE is missing" >&2
    exit 1
  }

  stage_start 4 "Restarting local node"
  "$CURRENT_EXE" start
  stage_done
}

invoke_install_script() {
  if [ -f "$SUPPORT_DIR/install.sh" ]; then
    "$SUPPORT_DIR/install.sh" "$@"
    return
  fi

  if [ -n "${0:-}" ] && [ -f "$0" ]; then
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    if [ -f "$script_dir/install.sh" ]; then
      "$script_dir/install.sh" "$@"
      return
    fi
  fi

  echo "missing local installer support at $SUPPORT_DIR/install.sh" >&2
  echo "run the bootstrap installer again to repair this installation" >&2
  exit 1
}

INSTALL_ROOT="$(resolve_install_root)"
CURRENT_LINK="$INSTALL_ROOT/current"
SUPPORT_DIR="$CURRENT_LINK/support"
CURRENT_EXE="$CURRENT_LINK/cloudagent"
CURRENT_NODE="$CURRENT_LINK/node"
CURRENT_AGENTD="$CURRENT_LINK/agentd"
restart_node=0
preflight_local_installer
if test_upgrade_restart_needed; then
  STAGE_TOTAL=4
  stop_node_if_running
  restart_node=1
else
  stage_start 1 "Checking local node"
  stage_done "(not running)"
fi

stage_start 3 "Running installer"
if invoke_install_script "$@"; then
  stage_done
else
  if [ "$restart_node" -eq 1 ] && [ -x "$CURRENT_EXE" ]; then
    stage_start 4 "Restoring local node"
    "$CURRENT_EXE" start || true
    stage_done "(best effort)"
  fi
  exit 1
fi

if [ "$restart_node" -eq 1 ]; then
  start_node_after_upgrade
fi
