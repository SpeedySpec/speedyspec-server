#!/usr/bin/env bash
# The crates published to crates.io: the ones named in scripts/publish-crates.txt (the PHP runtime crates other
# repositories build on), in the order they are published. Every other crate is private. Publishing is permanent, so
# the list is checked on every pull request and the release publishes only what it names.
#
#   cargo metadata --format-version 1 --no-deps > metadata.json
#   scripts/release-crates.sh check <metadata.json>
#       Fails, with one line per problem, unless every listed name is a workspace crate that may be published, every
#       other crate sets `publish = false`, and a listed crate's normal and build path dependencies are all listed and
#       carry a version (a published crate can only depend on published ones).
#   scripts/release-crates.sh pending <metadata.json>
#       Names the listed crates, in publishing order, whose workspace version crates.io does not have yet: what a
#       release publishes. A version already there is never published again, so a release that failed part-way can be
#       run again. Fails if the index cannot be read (a crate the index has never heard of is simply not published).
#
# SPEEDYSPEC_REPO overrides the repository root (where scripts/publish-crates.txt is read from), and
# SPEEDYSPEC_CRATES_INDEX the sparse index (https://index.crates.io; a file:// URL works too).
set -euo pipefail

usage() {
    echo "usage: scripts/release-crates.sh check|pending <metadata.json>" >&2
    exit 2
}

[ "$#" -eq 2 ] || usage
command="$1" metadata="$2"
repo="${SPEEDYSPEC_REPO:-$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)}"
list_file="$repo/scripts/publish-crates.txt"
[ -f "$list_file" ] || { echo "no $list_file" >&2; exit 1; }

# The listed names as a JSON array, in order, without blank lines or comments.
listed="$(grep -v -e '^[[:space:]]*$' -e '^[[:space:]]*#' "$list_file" | jq -R . | jq -sc .)"

case "$command" in
    check)
        problems="$(jq -r --argjson listed "$listed" '
            (.packages | map({key: .name, value: .}) | from_entries) as $crates
            | ($listed[] | select($crates[.] == null) | "\(.) is not a workspace crate"),
              (.packages[] | . as $p
                | if ($listed | index($p.name)) != null then
                    (if $p.publish == [] then "\($p.name) is listed but publish = false" else empty end),
                    ($p.dependencies[]
                        | select(.path != null and (.kind == null or .kind == "build"))
                        | . as $d
                        | if ($listed | index($d.name)) == null then "\($p.name) depends on unpublished \($d.name)"
                          elif $d.req == "*" then "\($p.name) depends on \($d.name) without a version"
                          else empty end)
                  elif $p.publish != [] then "\($p.name) must set publish = false"
                  else empty end)
        ' "$metadata")"
        if [ -n "$problems" ]; then
            printf '%s\n' "$problems" >&2
            exit 1
        fi
        ;;
    pending)
        index="${SPEEDYSPEC_CRATES_INDEX:-https://index.crates.io}"
        body="$(mktemp)"
        trap 'rm -f "$body"' EXIT
        for name in $(jq -r '.[]' <<<"$listed"); do
            version="$(jq -r --arg n "$name" '.packages[] | select(.name == $n) | .version' "$metadata")"
            [ -n "$version" ] || { echo "$name is not a workspace crate" >&2; exit 1; }
            # The sparse index's layout: 1/, 2/ and 3/<first letter>/ for short names, else <ab>/<cd>/<name>.
            lower="$(tr '[:upper:]' '[:lower:]' <<<"$name")"
            case "${#lower}" in
                1) path="1/$lower" ;;
                2) path="2/$lower" ;;
                3) path="3/${lower:0:1}/$lower" ;;
                *) path="${lower:0:2}/${lower:2:2}/$lower" ;;
            esac
            : > "$body"
            status=0
            code="$(curl -sS -o "$body" -w '%{http_code}' "$index/$path")" || status=$?
            if [ "$status" -eq 37 ] || [ "$code" = 404 ]; then
                # No such file (file://) or no such crate (https): nothing of it is published.
                echo "$name"
                continue
            fi
            if [ "$status" -ne 0 ] || { [ "$code" != 200 ] && [ "$code" != 000 ]; }; then
                echo "reading $index/$path failed (curl exit $status, HTTP $code)" >&2
                exit 1
            fi
            if ! jq -e -s --arg v "$version" 'any(.[]; .vers == $v)' "$body" >/dev/null; then
                echo "$name"
            fi
        done
        ;;
    *)
        usage
        ;;
esac
