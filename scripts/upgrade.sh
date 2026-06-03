#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
BOOTSTRAP_BRANCH="release-bootstrap"
BOOTSTRAP_RAW_BASE="https://raw.githubusercontent.com/$REPO/$BOOTSTRAP_BRANCH/bootstrap"
MAIN_RAW_BASE="https://raw.githubusercontent.com/$REPO/main/scripts"
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

curl_download() {
  url="$1"
  output="$2"
  label="$3"

  echo "$label"
  mkdir -p "$(dirname "$output")"
  if [ -t 2 ]; then
    curl --fail --location --progress-bar "$url" -o "$output"
  else
    curl -fsSL "$url" -o "$output"
  fi
}

resolve_bootstrap_url() {
  file_name="$1"
  bootstrap_url="$BOOTSTRAP_RAW_BASE/$file_name"
  if curl -fsSL -o /dev/null "$bootstrap_url" 2>/dev/null; then
    printf '%s\n' "$bootstrap_url"
  else
    printf '%s/%s\n' "$MAIN_RAW_BASE" "$file_name"
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
  install_url="$(resolve_bootstrap_url install.sh)"
  curl_download "$install_url" "$install_script" "Downloading installer script"
  chmod +x "$install_script"
  "$install_script" "$@"
}

restart_node=0
if stop_node_if_running; then
  restart_node=1
fi

echo "Installing updated CloudAgent version"
invoke_install_script "$@"

if [ "$restart_node" -eq 1 ]; then
  start_node_after_upgrade
fi
