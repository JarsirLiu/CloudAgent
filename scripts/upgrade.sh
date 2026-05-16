#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
CURRENT_LINK="$INSTALL_ROOT/current"
CURRENT_EXE="$CURRENT_LINK/cloudagent"
CURRENT_NODE="$CURRENT_LINK/node"
CURRENT_AGENTD="$CURRENT_LINK/agentd"
TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/cloudagent-upgrade-$$"

cleanup() {
  rm -rf "$WORK"
}

trap cleanup EXIT INT TERM

node_running() {
  [ -x "$CURRENT_NODE" ] || return 1
  if command -v pgrep >/dev/null 2>&1; then
    pgrep -f "$CURRENT_NODE" >/dev/null 2>&1
    return $?
  fi
  ps -ef 2>/dev/null | grep -F "$CURRENT_NODE" | grep -v grep >/dev/null 2>&1
}

stop_node_if_running() {
  [ -x "$CURRENT_NODE" ] || return 1
  if ! node_running; then
    return 1
  fi

  echo "stopping local node before upgrade"
  if command -v pkill >/dev/null 2>&1; then
    pkill -f "$CURRENT_AGENTD" >/dev/null 2>&1 || true
    pkill -f "$CURRENT_NODE" >/dev/null 2>&1 || true
  else
    ps -ef 2>/dev/null | grep -F "$CURRENT_AGENTD" | grep -v grep | awk '{print $2}' | xargs -r kill >/dev/null 2>&1 || true
    ps -ef 2>/dev/null | grep -F "$CURRENT_NODE" | grep -v grep | awk '{print $2}' | xargs -r kill >/dev/null 2>&1 || true
  fi
  return 0
}

start_node_after_upgrade() {
  [ -x "$CURRENT_EXE" ] || {
    echo "upgrade completed but $CURRENT_EXE is missing" >&2
    exit 1
  }

  echo "starting local node after upgrade"
  "$CURRENT_EXE" start
}

invoke_install_script() {
  if [ -n "${0:-}" ] && [ -f "$0" ]; then
    script_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
    if [ -f "$script_dir/install.sh" ]; then
      "$script_dir/install.sh" "$@"
      return
    fi
  fi

  mkdir -p "$WORK"
  install_script="$WORK/install.sh"
  curl -fsSL "https://raw.githubusercontent.com/$REPO/main/scripts/install.sh" -o "$install_script"
  chmod +x "$install_script"
  "$install_script" "$@"
}

restart_node=0
if stop_node_if_running; then
  restart_node=1
fi

invoke_install_script "$@"

if [ "$restart_node" -eq 1 ]; then
  start_node_after_upgrade
fi
