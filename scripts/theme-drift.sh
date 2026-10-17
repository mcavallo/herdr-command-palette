#!/bin/sh
# Compares the theme tables mirrored in src/domain/theme.rs with Herdr's
# source at a tag (tests/theme_drift.rs). Needs git and network.
# docs/development.md, "Theme tables".
#
# Usage: scripts/theme-drift.sh [tag]
#   tag  Herdr tag, e.g. v0.9.4; default: v<min_herdr_version> from
#        herdr-plugin.toml (the Herdr version the tables mirror)
set -eu

cd "$(dirname "$0")/.."

tag=${1:-}
if [ -z "$tag" ]; then
    min=$(sed -n 's/^min_herdr_version = "\(.*\)"$/\1/p' herdr-plugin.toml)
    [ -n "$min" ] || {
        echo "theme-drift: no min_herdr_version in herdr-plugin.toml" >&2
        exit 1
    }
    tag="v$min"
fi

src=$(mktemp -d)
trap 'rm -rf "$src"' EXIT

git -C "$src" init -q
git -C "$src" remote add origin https://github.com/herdrdev/herdr.git
git -C "$src" sparse-checkout set --no-cone /src/app/state.rs /src/app/mod.rs /src/config/theme.rs
git -C "$src" fetch -q --depth 1 --filter=blob:none origin "refs/tags/$tag"
git -C "$src" -c advice.detachedHead=false checkout -q FETCH_HEAD
echo "herdr $tag ($(git -C "$src" rev-parse --short HEAD))"
HERDR_SRC="$src" cargo test --locked --quiet --test theme_drift -- --ignored --nocapture
