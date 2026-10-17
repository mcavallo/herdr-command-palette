#!/bin/sh
# Copies the package version from Cargo.toml into herdr-plugin.toml.
# release-plz bumps only Cargo.toml; Herdr's marketplace reads the
# manifest (docs/releasing.md).
#
# Usage: scripts/sync-version.sh [--check]
#   --check  change nothing; exit 1 if the versions differ
set -eu

cd "$(dirname "$0")/.."

# First `version = "..."` line: the [package] one in Cargo.toml, the
# top-level one in the manifest.
version_of() {
    sed -n 's/^version = "\(.*\)"$/\1/p' "$1" | head -n 1
}

cargo_version=$(version_of Cargo.toml)
manifest_version=$(version_of herdr-plugin.toml)

if [ -z "$cargo_version" ] || [ -z "$manifest_version" ]; then
    echo "sync-version: no version line in Cargo.toml or herdr-plugin.toml" >&2
    exit 1
fi

if [ "$cargo_version" = "$manifest_version" ]; then
    exit 0
fi

if [ "${1:-}" = "--check" ]; then
    echo "herdr-plugin.toml version $manifest_version != Cargo.toml $cargo_version (run: mise run sync-version)" >&2
    exit 1
fi

# Only the first version line; a portable in-place edit (BSD and GNU sed differ).
tmp=$(mktemp)
awk -v v="$cargo_version" '
    !done && /^version = "/ { print "version = \"" v "\""; done = 1; next }
    { print }
' herdr-plugin.toml > "$tmp"
cat "$tmp" > herdr-plugin.toml
rm -f "$tmp"
echo "herdr-plugin.toml: $manifest_version -> $cargo_version"
