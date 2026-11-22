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
#
# SPEEDYSPEC_REPO overrides the repository root (where scripts/publish-crates.txt is read from).
set -euo pipefail

usage() {
    echo "usage: scripts/release-crates.sh check <metadata.json>" >&2
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
    *)
        usage
        ;;
esac
