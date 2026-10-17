#!/bin/sh
# Puts the plugin binary at target/release/, where herdr-plugin.toml's
# commands run it. Herdr runs this on `herdr plugin install` (the
# manifest's [[build]] command).
#
# Downloads the prebuilt binary of the version in Cargo.toml from the
# repository's GitHub release and checks its SHA-256. On an unknown
# platform or a failed download, builds from source with cargo instead.
# docs/releasing.md, "How users get the binary".
#
# Environment: HCP_INSTALL_REPO=owner/repo overrides the repository
# (default: the checkout's `origin` remote).
set -eu

cd "$(dirname "$0")/.."

name=herdr-command-palette
bin=target/release/$name

say() {
    printf '%s: %s\n' "$name" "$*"
}

# The first `key = "value"` line of Cargo.toml: the [package] one.
cargo_field() {
    sed -n "s/^$1 = \"\(.*\)\"\$/\1/p" Cargo.toml | head -n 1
}

version=$(cargo_field version)
rust_version=$(cargo_field rust-version)

# Release target triple of this machine; fails on an unknown one.
target_triple() {
    case "$(uname -s)/$(uname -m)" in
        Linux/x86_64 | Linux/amd64) echo x86_64-unknown-linux-musl ;;
        Linux/aarch64 | Linux/arm64) echo aarch64-unknown-linux-musl ;;
        Darwin/arm64 | Darwin/aarch64) echo aarch64-apple-darwin ;;
        Darwin/x86_64) echo x86_64-apple-darwin ;;
        *) return 1 ;;
    esac
}

# owner/repo of the GitHub repository this checkout came from.
repo_slug() {
    if [ -n "${HCP_INSTALL_REPO:-}" ]; then
        echo "$HCP_INSTALL_REPO"
        return
    fi
    url=$(git remote get-url origin 2>/dev/null) || return 1
    # https://github.com/owner/repo(.git) or git@github.com:owner/repo(.git)
    slug=$(printf '%s\n' "$url" | sed -n 's#^.*github\.com[:/]\([^/]*/[^/]*\)$#\1#p')
    slug=${slug%.git}
    [ -n "$slug" ] && echo "$slug"
}

# No retries: master only moves to a release after its assets are
# uploaded, and any failure falls back to cargo.
fetch() {
    curl -fsSL --connect-timeout 10 -o "$2" "$1"
}

sha256_of() {
    if command -v sha256sum >/dev/null 2>&1; then
        sha256sum "$1" | cut -d ' ' -f 1
    else
        shasum -a 256 "$1" | cut -d ' ' -f 1
    fi
}

# Downloads, checks and places the release binary; fails without
# touching target/release/ on any error.
install_prebuilt() {
    triple=$(target_triple) || {
        say "no prebuilt binary for $(uname -s) $(uname -m)"
        return 1
    }
    slug=$(repo_slug) || {
        say "can't tell the GitHub repository (set HCP_INSTALL_REPO=owner/repo)"
        return 1
    }
    archive=$name-$triple.tar.gz
    url=https://github.com/$slug/releases/download/v$version/$archive
    say "downloading v$version for $triple"
    tmp=$(mktemp -d) || {
        say "can't create a temp dir"
        return 1
    }
    if ! fetch "$url" "$tmp/$archive" || ! fetch "$url.sha256" "$tmp/$archive.sha256"; then
        say "download failed: $url"
        rm -rf "$tmp"
        return 1
    fi
    expected=$(cut -d ' ' -f 1 <"$tmp/$archive.sha256")
    actual=$(sha256_of "$tmp/$archive")
    if [ "$expected" != "$actual" ]; then
        say "checksum mismatch for $archive"
        rm -rf "$tmp"
        return 1
    fi
    if ! tar -xzf "$tmp/$archive" -C "$tmp" "$name"; then
        say "can't extract $archive"
        rm -rf "$tmp"
        return 1
    fi
    # `set -e` doesn't apply inside a function called with `||`: chain.
    # The last rename is in one directory, so atomic: a running palette
    # keeps the old file.
    if ! { mkdir -p target/release && chmod 755 "$tmp/$name" &&
        mv -f "$tmp/$name" "$bin.download" && mv -f "$bin.download" "$bin"; }; then
        say "can't write $bin"
        rm -rf "$tmp"
        return 1
    fi
    rm -rf "$tmp"
    say "installed $bin"
}

build_from_source() {
    # rustup's installer puts cargo here; Herdr's environment may not
    # have it on PATH.
    if ! command -v cargo >/dev/null 2>&1 && [ -f "$HOME/.cargo/env" ]; then
        # shellcheck disable=SC1091 # rustup's file, not in this repo
        . "$HOME/.cargo/env"
    fi
    if ! command -v cargo >/dev/null 2>&1; then
        say "cargo not found: install Rust $rust_version or newer (https://rustup.rs), then install the plugin again"
        exit 1
    fi
    say "building from source with cargo, takes a minute or two (needs Rust $rust_version or newer: rustup update)"
    cargo build --release --locked
}

install_prebuilt || build_from_source
