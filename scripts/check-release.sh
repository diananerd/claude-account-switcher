#!/bin/sh
# Release consistency: the version in Cargo.toml, the plugin manifest and the
# changelog must agree, and a release tag must name that same version.
#
#   scripts/check-release.sh           check the tree (CI, every push)
#   scripts/check-release.sh v1.2.3    also check a release tag

set -eu
cd "$(dirname "$0")/.."

fail() { printf 'check-release: %s\n' "$*" >&2; exit 1; }

cargo_v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
plugin_v=$(sed -n 's/.*"version": "\(.*\)".*/\1/p' plugin/.claude-plugin/plugin.json | head -1)
lock_v=$(awk '/^name = "claude-account-switcher"$/ { getline; sub(/^version = "/, ""); sub(/"$/, ""); print }' Cargo.lock)

[ -n "$cargo_v" ] || fail "no version in Cargo.toml"
[ "$plugin_v" = "$cargo_v" ] || fail "plugin.json has $plugin_v, Cargo.toml has $cargo_v"
[ "$lock_v" = "$cargo_v" ] || fail "Cargo.lock has $lock_v, Cargo.toml has $cargo_v (run cargo build)"
grep -q "^## \[$cargo_v\] - [0-9]\{4\}-[0-9]\{2\}-[0-9]\{2\}$" CHANGELOG.md \
  || fail "CHANGELOG.md has no '## [$cargo_v] - YYYY-MM-DD' entry"
if [ $# -gt 0 ]; then
  [ "$1" = "v$cargo_v" ] || fail "tag $1 does not match version $cargo_v"
fi
printf 'check-release: %s consistent\n' "$cargo_v"
