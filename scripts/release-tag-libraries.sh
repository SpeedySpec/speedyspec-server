#!/usr/bin/env bash
# Tags each library crate at its released version, after a release pull request is merged. Every crate is a
# release-please package (so a change in any crate is seen, and the cargo-workspace plugin bumps the server or the
# runtime when a crate it is built from is bumped), but only the server and the runtime publish GitHub releases.
# release-please finds where a package was last released from its tag, so without these tags it would count every
# commit since the start again on every run. Lightweight tags only: libraries get no GitHub release.
#
#   scripts/release-tag-libraries.sh <commit>
#
# Creates the missing <crate>-v<version> tags (the versions in .release-please-manifest.json) at <commit>, locally,
# and prints their names, one per line, for the caller to push. SPEEDYSPEC_REPO overrides the repository root.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: scripts/release-tag-libraries.sh <commit>" >&2
    exit 2
fi
commit="$1"
repo="${SPEEDYSPEC_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"

jq -r 'to_entries[] | "\(.key) \(.value)"' "$repo/.release-please-manifest.json" | while read -r path version; do
    name="${path##*/}"
    case "$name" in
        speedyspec-server|speedyspec-php-runtime) continue ;;
    esac
    tag="$name-v$version"
    if ! git -C "$repo" rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
        git -C "$repo" tag "$tag" "$commit"
        echo "$tag"
    fi
done
