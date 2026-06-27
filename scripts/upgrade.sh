#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
SCRIPT_BASE_URL="${CLOUDAGENT_SCRIPT_BASE_URL:-https://github.com/$REPO/releases/latest/download}"
SCRIPT_FALLBACK_URL="${CLOUDAGENT_SCRIPT_FALLBACK_URL:-https://raw.githubusercontent.com/$REPO/main/scripts}"
INSTALL_ROOT="${CLOUDAGENT_INSTALL_ROOT:-$HOME/.local/lib/cloudagent}"
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

curl_download() {
  url="$1"
  output="$2"
  mkdir -p "$(dirname "$output")"
  if [ -t 2 ]; then
    if curl --fail --location --progress-bar "$url" -o "$output"; then
      return 0
    fi
  else
    if curl -fsSL "$url" -o "$output"; then
      return 0
    fi
  fi

  if command -v wget >/dev/null 2>&1; then
    if [ -t 2 ]; then
      if wget --show-progress -O "$output" "$url"; then
        return 0
      fi
    else
      if wget -q -O "$output" "$url"; then
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
    ps -ef 2>/dev/null | grep -F "$CURRENT_AGENTD" | grep -v grep | awk '{print $2}' | xargs -r kill >/dev/null 2>&1 || true
    ps -ef 2>/dev/null | grep -F "$CURRENT_NODE" | grep -v grep | awk '{print $2}' | xargs -r kill >/dev/null 2>&1 || true
  fi
  stage_done "(stopped)"
  return 0
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

  mkdir -p "$WORK"
  install_script="$WORK/install.sh"
  stage_start 2 "Downloading installer script"
  download_remote_script "install.sh" "$install_script"
  chmod +x "$install_script"
  stage_done
  "$install_script" "$@"
}

restart_node=0
if test_upgrade_restart_needed; then
  STAGE_TOTAL=4
  stop_node_if_running
  restart_node=1
else
  stage_start 1 "Checking local node"
  stage_done "(not running)"
fi

stage_start 3 "Running installer"
invoke_install_script "$@"
stage_done

if [ "$restart_node" -eq 1 ]; then
  start_node_after_upgrade
fi
