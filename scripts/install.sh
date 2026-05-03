#!/usr/bin/env sh
set -eu

PREFIX="$HOME/.local/bin"
BINARIES="cli agentd gatewayd cloudagent"
SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

find_all() {
  cmd="$1"
  if command -v which >/dev/null 2>&1; then
    which -a "$cmd" 2>/dev/null | awk '!seen[$0]++'
  fi
}

warn_conflicts() {
  for b in $BINARIES; do
    paths=$(find_all "$b" || true)
    count=$(printf "%s\n" "$paths" | sed '/^$/d' | wc -l | tr -d ' ')
    if [ "$count" -gt 1 ]; then
      echo "warning: multiple '$b' found in PATH:" >&2
      printf "%s\n" "$paths" >&2
    fi
  done
}

mkdir -p "$PREFIX"

CANDIDATES="\
$ROOT_DIR/cli\
 $ROOT_DIR/agentd\
 $ROOT_DIR/gatewayd\
 $ROOT_DIR/scripts/cloudagent\
 $ROOT_DIR/target/release/cli\
 $ROOT_DIR/target/release/agentd\
 $ROOT_DIR/target/release/gatewayd"

installed=0
for bin in $CANDIDATES; do
  if [ -f "$bin" ]; then
    name=$(basename "$bin")
    install -m 755 "$bin" "$PREFIX/$name"
    installed=1
    echo "installed: $PREFIX/$name"
  fi
done

if [ "$installed" -ne 1 ]; then
  echo "no binaries found. expected cli/agentd/gatewayd or scripts/cloudagent" >&2
  echo "tip: build first with: cargo build --release -p cli -p agentd -p gatewayd" >&2
  exit 1
fi

add_path_line='export PATH="$HOME/.local/bin:$PATH"'
case ":$PATH:" in
  *":$HOME/.local/bin:"*) : ;;
  *)
    for rc in "$HOME/.bashrc" "$HOME/.zshrc"; do
      if [ -f "$rc" ] && ! grep -Fq "$add_path_line" "$rc"; then
        printf "\n%s\n" "$add_path_line" >> "$rc"
        echo "updated PATH in $rc"
      fi
done

if [ ! -f "$PREFIX/cloudagent" ]; then
  echo "warning: cloudagent launcher missing; install may be incomplete" >&2
fi
    echo "open a new terminal or run: export PATH=\"$HOME/.local/bin:$PATH\""
    ;;
esac

warn_conflicts

echo "done. active install prefix: $PREFIX"
echo "try: cloudagent start"
