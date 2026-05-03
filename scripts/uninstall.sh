#!/usr/bin/env sh
set -eu

PREFIX="$HOME/.local/bin"
BINARIES="cli agentd gatewayd cloudagent"

for name in $BINARIES; do
  target="$PREFIX/$name"
  if [ -f "$target" ]; then
    rm -f "$target"
    echo "removed: $target"
  fi
done

for name in $BINARIES; do
  if command -v which >/dev/null 2>&1; then
    paths=$(which -a "$name" 2>/dev/null | awk '!seen[$0]++' || true)
    first=$(printf "%s\n" "$paths" | sed -n '1p')
    if [ -n "$first" ] && [ "$first" != "$PREFIX/$name" ]; then
      echo "notice: another '$name' still exists at: $first"
    fi
  fi
done

echo "done. PATH lines in shell rc files are kept as-is."
