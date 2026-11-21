#!/usr/bin/env bash
# Says whether a component (speedyspec-server or speedyspec-php-runtime) has a release that is merged but not yet
# tagged: the state right after its release-please pull request is merged, which bumps the crate's version and its
# entry in .release-please-manifest.json together. The Release workflow builds that component first in this state,
# so a release is published only once its binaries exist. The manifest's 0.0.0 means "never released": before the
# first release pull request, ordinary pushes build nothing.
#
#   scripts/release-pending.sh <component>
#
# Prints, for $GITHUB_OUTPUT:
#   pending=true|false
#   tag=<component>-v<version>
#   version=<version>
#   commit=<the commit that last changed .release-please-manifest.json: the merged release pull request>
# The release is built and tagged at that commit, so a retry after a failed build builds what release-please tags.
# Tags and enough history must have been fetched (git fetch --tags, and a checkout with fetch-depth: 0). SPEEDYSPEC_REPO overrides the repository root.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: scripts/release-pending.sh speedyspec-server|speedyspec-php-runtime" >&2
    exit 2
fi
component="$1"
case "$component" in
    speedyspec-server|speedyspec-php-runtime) ;;
    *)
        echo "release-pending.sh: unknown component $component" >&2
        exit 2
        ;;
esac

repo="${SPEEDYSPEC_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
# The package's own version: the first `version =` line, which is the [package] table's.
version="$(sed -n 's/^version = "\(.*\)"$/\1/p' "$repo/crates/$component/Cargo.toml" | head -n 1)"
released="$(jq -r --arg key "crates/$component" '.[$key] // "0.0.0"' "$repo/.release-please-manifest.json")"
tag="$component-v$version"

if [ -z "$version" ]; then
    echo "release-pending.sh: no version in crates/$component/Cargo.toml" >&2
    exit 1
fi
if [ "$released" = "0.0.0" ] || [ "$released" != "$version" ]; then
    pending=false
elif git -C "$repo" rev-parse -q --verify "refs/tags/$tag" >/dev/null; then
    pending=false
else
    pending=true
fi
commit="$(git -C "$repo" log -1 --format=%H -- .release-please-manifest.json)"
if [ -z "$commit" ]; then
    echo "release-pending.sh: no commit has changed .release-please-manifest.json (a shallow checkout?)" >&2
    exit 1
fi
printf 'pending=%s\ntag=%s\nversion=%s\ncommit=%s\n' "$pending" "$tag" "$version" "$commit"
