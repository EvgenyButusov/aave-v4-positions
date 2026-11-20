#!/usr/bin/env bash
#
# Writes what crates/aave-abi vendors from an unpacked @aave-dao/aave-address-book,
# reading it as text rather than running it:
#
#   ./scripts/extract-aave-abi.sh <package dir> <out dir>
#
# Each ABI is the one generated array literal in dist/abis/<name>.js, which differs
# from JSON only in its unquoted keys; jq both checks the result is JSON and prints
# it. The addresses come from the book's own Solidity, src/AaveV4Ethereum.sol.
#
# Needs bash, sed, tr, grep and jq.

set -euo pipefail

if [ $# -ne 2 ]; then
  echo "usage: extract-aave-abi.sh <package dir> <out dir>" >&2
  exit 2
fi
readonly package=$1 out=$2

abi() {
  local file="$package/dist/abis/$1.js"
  sed -n "/^var $1_ABI = \[\$/,/^\];\$/p" "$file" |
    sed -e '1s/.*/[/' -e '$s/.*/]/' |
    sed -E 's/^( *)([A-Za-z_][A-Za-z0-9_]*): /\1"\2": /' |
    jq .
}

# Exactly one `<Type> internal constant <NAME> = <Type>(0x…)`, which the generator may wrap.
constant() {
  local found
  found=$(tr -s ' \n' ' ' <"$package/src/AaveV4Ethereum.sol" |
    grep -oE "constant $1 = [A-Za-z]+\(0x[0-9a-fA-F]{40}\)" |
    grep -oE '0x[0-9a-fA-F]{40}') || true
  if [ "$(printf '%s' "$found" | grep -c .)" -ne 1 ]; then
    echo "AaveV4Ethereum.sol has no single $1" >&2
    return 1
  fi
  printf '%s' "$found"
}

# Everything is read before anything is written, so a failure leaves the vendored files as they were.
staged=$(mktemp -d)
trap 'rm -rf "$staged"' EXIT

for interface in ISpokeV4 IHubV4 IAaveOracleV4; do
  abi "$interface" >"$staged/$interface.json"
  if ! jq -e 'type == "array" and length > 0' "$staged/$interface.json" >/dev/null; then
    echo "dist/abis/$interface.js holds no ABI" >&2
    exit 1
  fi
done

# One substitution per assignment: `set -e` sees neither one inside another command's arguments
# nor any but the last of several in one assignment.
name=$(jq -er .name "$package/package.json")
version=$(jq -er .version "$package/package.json")
main_spoke=$(constant MAIN_SPOKE)
main_spoke_oracle=$(constant MAIN_SPOKE_ORACLE)
core_hub=$(constant CORE_HUB)

jq -n \
  --arg source "$name@$version" \
  --arg mainSpoke "$main_spoke" \
  --arg mainSpokeOracle "$main_spoke_oracle" \
  --arg coreHub "$core_hub" \
  '{source: $source, mainSpoke: $mainSpoke, mainSpokeOracle: $mainSpokeOracle, coreHub: $coreHub}' \
  >"$staged/deployment.json"

cp "$staged"/*.json "$out"/
