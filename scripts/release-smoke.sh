#!/usr/bin/env bash
# Checks a release binary before it is published: its `manifest` must name the version being released (and, for the
# server, the `server` app; the runtime's manifest has no app but must offer a contract and report the pinned
# libsodium), and on Linux it must be a static executable.
#
#   scripts/release-smoke.sh <binary> speedyspec-server|speedyspec-php-runtime <version>
set -euo pipefail

if [ "$#" -ne 3 ]; then
    echo "usage: scripts/release-smoke.sh <binary> speedyspec-server|speedyspec-php-runtime <version>" >&2
    exit 2
fi
binary="$1" component="$2" version="$3"

fail() {
    echo "release-smoke.sh: $binary: $*" >&2
    exit 1
}

manifest="$("$binary" manifest)" || fail "\`manifest\` failed"
got="$(jq -r '.version // empty' <<<"$manifest")" || fail "the manifest is not JSON: $manifest"
[ "$got" = "$version" ] || fail "version is ${got:-missing}, not $version"
case "$component" in
    speedyspec-server)
        app="$(jq -r '.app // empty' <<<"$manifest")"
        [ "$app" = "server" ] || fail "app is ${app:-missing}, not server"
        ;;
    speedyspec-php-runtime)
        contracts="$(jq '.contracts | length' <<<"$manifest")"
        [ "$contracts" -gt 0 ] || fail "the manifest offers no contracts"
        # WordPress needs libsodium, and the runtime guarantees it at the version build-php.sh pins.
        pinned="$(sed -n 's/^libsodium_version="\(.*\)"$/\1/p' "$(dirname "${BASH_SOURCE[0]}")/build-php.sh")"
        [ -n "$pinned" ] || fail "scripts/build-php.sh pins no libsodium_version"
        sodium="$(jq -r '.sodium // empty' <<<"$manifest")"
        [ "$sodium" = "$pinned" ] || fail "libsodium is ${sodium:-missing}, not $pinned"
        ;;
    *)
        echo "release-smoke.sh: unknown component $component" >&2
        exit 2
        ;;
esac
if [ "$(uname -s)" = "Linux" ]; then
    kind="$(file -b "$binary")"
    case "$kind" in
        *"statically linked"*|*"static-pie linked"*) ;;
        *) fail "not a static executable: $kind" ;;
    esac
fi
echo "release-smoke.sh: $binary: $component $version ok"
