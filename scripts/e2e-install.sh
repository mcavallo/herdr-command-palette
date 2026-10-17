#!/bin/sh
# End-to-end check of `herdr plugin install` in a throwaway container:
# a fresh Herdr installs this plugin from GitHub, runs its [[build]]
# script, and the installed binary has to run. Nothing touches your own
# Herdr: the container gets Herdr's release binary and nothing from your
# home directory. docs/development.md, "End-to-end install check".
#
# Usage: scripts/e2e-install.sh [download|cargo|clean] [herdr-version]
#   download  (default) Debian without Rust: the release binary must be
#             downloaded; needs a public repository with a release for
#             the version on the installed branch
#   cargo     Rust image at Cargo.toml's rust-version: the download fails
#             or is skipped and cargo must build it
#   clean     remove the two images above (they stay cached between
#             runs; the Rust one is about 1 GB)
#   herdr-version  e.g. 0.9.5; default: Herdr's latest release
#
# Environment:
#   HCP_E2E_REPO     owner/repo to install (default: the `origin` remote)
#   HCP_E2E_RUNTIME  container runtime (default: podman)
set -eu

cd "$(dirname "$0")/.."

mode=${1:-download}
herdr_version=${2:-latest}
runtime=${HCP_E2E_RUNTIME:-podman}

say() {
    printf 'e2e-install: %s\n' "$*"
}

fail() {
    say "$*" >&2
    exit 1
}

rust_version=$(sed -n 's/^rust-version = "\(.*\)"$/\1/p' Cargo.toml | head -n 1)
download_image=docker.io/library/debian:stable-slim
cargo_image=docker.io/library/rust:$rust_version-slim

command -v "$runtime" >/dev/null 2>&1 || fail "$runtime not found (set HCP_E2E_RUNTIME)"

case "$mode" in
    download) image=$download_image ;;
    cargo) image=$cargo_image ;;
    clean)
        for image in "$download_image" "$cargo_image"; do
            if "$runtime" image exists "$image" 2>/dev/null ||
                "$runtime" image inspect "$image" >/dev/null 2>&1; then
                "$runtime" rmi "$image" >/dev/null && say "removed $image"
            else
                say "not present: $image"
            fi
        done
        exit 0
        ;;
    *) fail "unknown mode '$mode' (download, cargo or clean)" ;;
esac

if [ -n "${HCP_E2E_REPO:-}" ]; then
    repo=$HCP_E2E_REPO
else
    repo=$(git remote get-url origin 2>/dev/null | sed -n 's#^.*github\.com[:/]\([^/]*/[^/]*\)$#\1#p')
    repo=${repo%.git}
    [ -n "$repo" ] || fail "origin is not a GitHub remote (set HCP_E2E_REPO=owner/repo)"
fi

case "$(uname -m)" in
    x86_64 | amd64) herdr_asset=herdr-linux-x86_64 ;;
    aarch64 | arm64) herdr_asset=herdr-linux-aarch64 ;;
    *) fail "no Herdr Linux release for $(uname -m)" ;;
esac
if [ "$herdr_version" = latest ]; then
    herdr_url=https://github.com/herdrdev/herdr/releases/latest/download/$herdr_asset
else
    herdr_url=https://github.com/herdrdev/herdr/releases/download/v${herdr_version#v}/$herdr_asset
fi

work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT INT TERM

say "downloading Herdr ($herdr_version): $herdr_url"
curl -fsSL -o "$work/herdr" "$herdr_url" || fail "can't download Herdr $herdr_version"
chmod 755 "$work/herdr"
# Herdr publishes no checksums or attestations: print the hash for the record.
if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$work/herdr"
else
    shasum -a 256 "$work/herdr"
fi

# Runs inside the container (scripts/e2e-inner.sh).
cp scripts/e2e-inner.sh "$work/inner.sh"

say "$mode mode: installing $repo in $image"
# :Z relabels the mount for SELinux.
"$runtime" run --rm \
    -e MODE="$mode" -e REPO="$repo" \
    -v "$work:/e2e:ro,Z" \
    "$image" sh /e2e/inner.sh
