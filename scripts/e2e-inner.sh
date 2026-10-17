#!/bin/sh
# The container side of scripts/e2e-install.sh, which copies it into the
# container's /e2e mount with the Herdr binary and runs it as root, in a
# fresh HOME.
#
# Environment (set by e2e-install.sh):
#   MODE   download or cargo: the build path the install must take
#   REPO   owner/repo to install
set -eu
say() { printf 'e2e-install (container): %s\n' "$*"; }
fail() { say "FAIL: $*" >&2; exit 1; }
# It installs packages system-wide: never on the host.
[ -f /e2e/herdr ] || fail "only runs in scripts/e2e-install.sh's container"

say "installing git$([ "$MODE" = download ] && echo ', curl')"
apt-get update -qq >/dev/null
if [ "$MODE" = download ]; then
    apt-get install -y -qq --no-install-recommends git curl ca-certificates >/dev/null 2>&1
    ! command -v cargo >/dev/null 2>&1 || fail "cargo in a download-mode image"
else
    apt-get install -y -qq --no-install-recommends git >/dev/null 2>&1
fi
install -m 755 /e2e/herdr /usr/local/bin/herdr
say "$(herdr --version)"

say "herdr plugin install $REPO --yes"
start=$(date +%s)
herdr plugin install "$REPO" --yes
say "install took $(($(date +%s) - start)) s"

herdr plugin list | grep -q "mmc.command-palette" || fail "plugin not listed"
bin=$(find "$HOME" -path '*/checkout/target/release/herdr-command-palette' -type f | head -n 1)
[ -n "$bin" ] || fail "no installed binary"
checkout=${bin%/target/release/herdr-command-palette}
expected="herdr-command-palette $(sed -n 's/^version = "\(.*\)"$/\1/p' "$checkout/herdr-plugin.toml" | head -n 1)"
actual=$("$bin" --version)
[ "$actual" = "$expected" ] || fail "binary says '$actual', manifest '$expected'"
say "binary: $actual"

# cargo leaves target/release/deps; the download only puts the binary there.
if [ -d "$checkout/target/release/deps" ]; then how=cargo; else how=download; fi
[ "$how" = "$MODE" ] || fail "expected the $MODE path, got $how"
say "OK: $MODE path"
