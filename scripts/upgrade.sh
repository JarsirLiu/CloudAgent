#!/usr/bin/env sh
set -eu

REPO="JarsirLiu/CloudAgent"
PREFIX="$HOME/.local/bin"
TMPDIR="${TMPDIR:-/tmp}"
WORK="$TMPDIR/cloudagent-upgrade-$$"
mkdir -p "$WORK"
trap 'rm -rf "$WORK"' EXIT INT TERM

latest_json=$(curl -fsSL "https://api.github.com/repos/$REPO/releases/latest")
url=$(printf "%s" "$latest_json" | sed -n 's/.*"browser_download_url"[[:space:]]*:[[:space:]]*"\([^"]*\)".*/\1/p' | grep -E 'linux|darwin|macos' | head -n 1)

if [ -z "$url" ]; then
  echo "no suitable release asset found automatically." >&2
  echo "please download manually from: https://github.com/$REPO/releases" >&2
  exit 1
fi

echo "downloading: $url"
asset="$WORK/asset"
curl -fL "$url" -o "$asset"

case "$url" in
  *.tar.gz|*.tgz)
    tar -xzf "$asset" -C "$WORK"
    ;;
  *.zip)
    unzip -q "$asset" -d "$WORK"
    ;;
  *)
    echo "unsupported asset format: $url" >&2
    exit 1
    ;;
esac

found=0
for b in cli agentd node cloudagent; do
  candidate=$(find "$WORK" -type f -name "$b" | head -n 1 || true)
  if [ -n "$candidate" ]; then
    mkdir -p "$PREFIX"
    install -m 755 "$candidate" "$PREFIX/$b"
    echo "upgraded: $PREFIX/$b"
    found=1
  fi
done

launcher=$(find "$WORK" -type f -name "cloudagent" | head -n 1 || true)
if [ -n "$launcher" ]; then
  install -m 755 "$launcher" "$PREFIX/cloudagent"
  echo "upgraded: $PREFIX/cloudagent"
fi

if [ "$found" -ne 1 ]; then
  echo "upgrade failed: no cli/agentd/node/cloudagent binaries in release asset" >&2
  exit 1
fi

echo "done. restart terminal if needed."
