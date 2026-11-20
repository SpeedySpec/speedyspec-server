#!/usr/bin/env bash
# Writes a component's release index (the JSON managed mode reads from an app's `source`) on stdout, built from the
# repository's GitHub releases: the index is derived data, rebuilt in full on every publish.
#
#   scripts/release-index.sh speedyspec-server server
#   scripts/release-index.sh speedyspec-php-runtime php-runtime
#
# A release is listed when its tag is <component>-v<version>, it is neither a draft nor a pre-release, and every
# target has its binary, its .minisig and a line in its SHA256SUMS. Others are skipped, with the reason on stderr.
#
# Releases come from `gh api --paginate repos/$GITHUB_REPOSITORY/releases`, or from the file $RELEASE_INDEX_RELEASES;
# each SHA256SUMS from `gh release download`, or from $RELEASE_INDEX_SUMS_DIR/<tag>.SHA256SUMS.
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: scripts/release-index.sh <component> <app-id>" >&2
    exit 2
fi
component="$1" app="$2"
triples="x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin"

work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

if [ -n "${RELEASE_INDEX_RELEASES:-}" ]; then
    cp "$RELEASE_INDEX_RELEASES" "$work/pages.json"
else
    gh api --paginate "repos/$GITHUB_REPOSITORY/releases" > "$work/pages.json"
fi
# --paginate prints one JSON array per page.
jq -s 'add // []' "$work/pages.json" > "$work/releases.json"

sums() {
    if [ -n "${RELEASE_INDEX_SUMS_DIR:-}" ]; then
        cat "$RELEASE_INDEX_SUMS_DIR/$1.SHA256SUMS" 2>/dev/null || true
    else
        gh release download "$1" --repo "$GITHUB_REPOSITORY" --pattern SHA256SUMS --output - 2>/dev/null || true
    fi
}

entries="$work/entries.jsonl"
: > "$entries"
jq -c --arg prefix "$component-v" '
    .[]
    | select(.tag_name | startswith($prefix))
' "$work/releases.json" | while IFS= read -r release; do
    tag="$(jq -r .tag_name <<<"$release")"
    if ! [[ "${tag#"$component-v"}" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
        echo "release-index: skipping $tag: not a full release version" >&2
        continue
    fi
    if [ "$(jq -r .draft <<<"$release")" = "true" ]; then
        echo "release-index: skipping $tag: a draft" >&2
        continue
    fi
    if [ "$(jq -r .prerelease <<<"$release")" = "true" ]; then
        echo "release-index: skipping $tag: a pre-release" >&2
        continue
    fi
    sums_text="$(sums "$tag")"
    artifacts="[]"
    why=""
    for triple in $triples; do
        name="$component-$triple"
        url="$(jq -r --arg n "$name" '.assets[]? | select(.name == $n) | .browser_download_url' <<<"$release")"
        sig="$(jq -r --arg n "$name.minisig" '.assets[]? | select(.name == $n) | .browser_download_url' <<<"$release")"
        sha="$(awk -v n="$name" '$2 == n || $2 == "*" n { print tolower($1) }' <<<"$sums_text")"
        if [ -z "$url" ]; then why="no $name"; break; fi
        if [ -z "$sig" ]; then why="no $name.minisig"; break; fi
        if [ -z "$sha" ]; then why="no SHA256SUMS line for $name"; break; fi
        artifacts="$(jq -c --arg t "$triple" --arg u "$url" --arg s "$sha" --arg m "$sig" \
            '. + [{target: $t, url: $u, sha256: $s, minisig: $m}]' <<<"$artifacts")"
    done
    if [ -n "$why" ]; then
        echo "release-index: skipping $tag: incomplete ($why)" >&2
        continue
    fi
    jq -c --arg v "${tag#"$component-v"}" --argjson a "$artifacts" \
        '{version: $v, published: (.published_at // ""), artifacts: $a}' <<<"$release" >> "$entries"
done

# Newest first. Pre-releases are never listed, so versions compare as three numbers.
jq -s --arg app "$app" '{
    app: $app,
    releases: (sort_by(.version | split(".") | map(tonumber)) | reverse)
}' "$entries"
