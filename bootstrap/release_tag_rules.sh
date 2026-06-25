#!/usr/bin/env sh
set -eu

is_semver_tag() {
  case "$1" in
    v*)
      printf '%s\n' "$1" | grep -Eq '^v(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)\.(0|[1-9][0-9]*)(-[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?(\+[0-9A-Za-z-]+(\.[0-9A-Za-z-]+)*)?$'
      ;;
    *)
      return 1
      ;;
  esac
}

normalize_release_tag() {
  version="$1"
  version="$(printf '%s' "$version" | tr -d '[:space:]')"
  case "$version" in
    '')
      echo "invalid release version: $1" >&2
      exit 1
      ;;
    v*)
      release_tag="$version"
      ;;
    *)
      release_tag="v$version"
      ;;
  esac

  if is_semver_tag "$release_tag"; then
    printf '%s\n' "$release_tag"
    return 0
  fi

  echo "invalid release version: $1" >&2
  exit 1
}

run_self_test() {
  valid_tags="v0.1.0 v1.2.3 v1.2.3-beta.1 v1.2.3+build.7 v1.2.3-beta.1+build.7"
  invalid_tags="v v1 v1.2 1.2.3 v01.2.3 v1.02.3 v1.2.03 v1.2.3- v1.2.3+"

  for tag in $valid_tags; do
    if ! is_semver_tag "$tag"; then
      echo "expected valid tag to pass: $tag" >&2
      exit 1
    fi
  done

  for tag in $invalid_tags; do
    if is_semver_tag "$tag"; then
      echo "expected invalid tag to fail: $tag" >&2
      exit 1
    fi
  done

  if [ "$(normalize_release_tag 1.2.3)" != "v1.2.3" ]; then
    echo "normalize_release_tag failed for 1.2.3" >&2
    exit 1
  fi

  if [ "$(normalize_release_tag v1.2.3-beta.1)" != "v1.2.3-beta.1" ]; then
    echo "normalize_release_tag failed for v1.2.3-beta.1" >&2
    exit 1
  fi

  echo "release_tag_rules.sh self-test passed"
}

if [ "$(basename -- "$0")" = "release_tag_rules.sh" ]; then
  case "${1:-}" in
    --self-test)
      run_self_test
      ;;
  esac
fi
