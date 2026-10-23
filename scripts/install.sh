#!/bin/sh
# Bootstrap fer without Python, Lua, root, or Cargo.
# Usage: sh scripts/install.sh v0.1.0 [INSTALL_PREFIX]
set -eu
fail() { printf 'fer installer: %s\n' "$*" >&2; exit 1; }
[ "$#" -ge 1 ] && [ "$#" -le 2 ] || fail "usage: install.sh VERSION [PREFIX]"
version=$1
printf '%s\n' "$version" | grep -Eq '^v[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.-]+)?$' ||
    fail "expected explicit semver release, such as v0.1.0"
[ "$(uname -s)" = Linux ] || fail "only Linux is supported by this installer"
[ "$(uname -m)" = x86_64 ] || fail "only x86_64 is supported by this installer"
for tool in curl sha256sum tar install; do
    command -v "$tool" >/dev/null 2>&1 || fail "$tool not installed"
done
if [ "$#" -eq 2 ]; then prefix=$2; else prefix="$HOME/.local"; fi
[ -n "$prefix" ] || fail "installation prefix is empty"
base=$(printenv FERRUA_RELEASE_BASE || true)
if [ -z "$base" ]; then
    base=https://github.com/sguzman/ferrua/releases/download
else
    printf '%s\n' "$base" | grep -Eq '^http://127\.0\.0\.1:[0-9]+$' ||
        fail "download server must be official GitHub releases or loopback fixture"
fi

archive="fer-$version-linux-x86_64.tar.gz"
tmp=$(mktemp -d) || fail "could not allocate temporary directory"
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
url="$base/$version"
(
    cd "$tmp"
    curl --fail --silent --show-error --location --max-redirs 5 \
        --proto '=https,http' --proto-redir '=https,http' \
        -o "$archive" "$url/$archive" || fail "unable to fetch release archive"
    curl --fail --silent --show-error --location --max-redirs 5 \
        --proto '=https,http' --proto-redir '=https,http' \
        -o "$archive.sha256" "$url/$archive.sha256" ||
        fail "unable to fetch release checksum"
    record=$(cat "$archive.sha256")
    printf '%s\n' "$record" | grep -Eq "^[0-9a-f]{64}  $archive$" ||
        fail "invalid release checksum manifest"
    sha256sum --check "$archive.sha256" >/dev/null || fail "SHA-256 mismatch"
    tar -tzf "$archive" >/dev/null || fail "archive is corrupt"
    tar -xzf "$archive" --no-same-owner --no-same-permissions fer ||
        fail "archive does not contain fer"
    [ -f fer ] && [ ! -L fer ] || fail "extracted fer is not a regular executable file"
)
mkdir -p "$prefix/bin"
install -m 0755 "$tmp/fer" "$prefix/bin/fer"
printf 'Installed %s to %s/bin/fer\n' "$version" "$prefix"
