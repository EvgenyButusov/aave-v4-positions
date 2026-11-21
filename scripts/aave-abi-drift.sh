#!/usr/bin/env bash
#
# crates/aave/abi is exactly what the pinned @aave-dao/aave-address-book publishes.
#
# Re-extracts from that version and fails on any difference, so an upstream change
# is a red build rather than a silent divergence, and "taken from the address book
# rather than transcribed" stays true. The version is the `source` the vendored
# deployment.json records; to move to another, change it there and in the pnpm
# catalog, run this, and review the diff it leaves.
#
# Needs bash, curl, tar and jq. ADDRESS_BOOK_DIR names an unpacked copy of the
# package to read instead of fetching one, such as the one under node_modules.

set -euo pipefail
cd "$(dirname "$0")/.."

readonly VENDORED=crates/aave/abi

source=$(jq -r .source "$VENDORED/deployment.json")
name=${source%@*}
version=${source##*@}

# The TypeScript reads the same book while it runs; the two may not follow different versions.
if [ -f pnpm-workspace.yaml ] &&
  ! grep -qF "'$name': $version" pnpm-workspace.yaml; then
  echo "pnpm-workspace.yaml pins a different $name than $source" >&2
  exit 1
fi

package=${ADDRESS_BOOK_DIR:-}
if [ -z "$package" ]; then
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  curl -fsSL "https://registry.npmjs.org/$name/-/${name#*/}-$version.tgz" | tar -xz -C "$work"
  package="$work/package"
fi

./scripts/extract-aave-abi.sh "$package" "$VENDORED"

if ! git diff --exit-code --stat -- "$VENDORED"; then
  echo "$VENDORED differs from what $source publishes" >&2
  exit 1
fi
