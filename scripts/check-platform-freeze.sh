#!/usr/bin/env bash
# Usage: scripts/check-platform-freeze.sh <base-ref>
#
# The paths in scripts/platform-paths.txt moved to SpeedySpec/speedyspec-platform. Until the server depends on the
# published crates, they must not change here. Prints every path that changed between <base-ref> and HEAD under a
# frozen entry, and exits 1 if there is any. An entry ending in `/` matches as a directory prefix; any other entry
# matches one file exactly. Renames count as a deletion plus an addition, so moving a file out of a frozen directory
# is a change too.
#
# The list is read from <base-ref>, not from the working tree, so a pull request cannot drop an entry and change
# that path together.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <base-ref>" >&2
    exit 2
fi
base="$1"
root="$(git rev-parse --show-toplevel)"

if ! list="$(git -C "$root" show "$base:scripts/platform-paths.txt" 2>/dev/null)"; then
    echo "$base has no scripts/platform-paths.txt (or $base is not a commit)" >&2
    exit 2
fi

entries=()
while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
    '' | '#'*) continue ;;
    esac
    entries+=("$line")
done <<<"$list"

# NUL-delimited, so a path git would C-quote (non-ASCII, a newline, a quote) is compared as it is. Written to a file
# first so that a failing diff fails the script (set -e) instead of looking like no changes.
changes="$(mktemp)"
trap 'rm -f "$changes"' EXIT
git -C "$root" diff -z --name-only --no-renames "$base"...HEAD >"$changes"

frozen=0
while IFS= read -r -d '' changed; do
    for entry in ${entries[@]+"${entries[@]}"}; do
        if [ "${entry%/}" != "$entry" ]; then
            [ "${changed#"$entry"}" != "$changed" ] || continue
        else
            [ "$changed" = "$entry" ] || continue
        fi
        echo "$changed: moved to speedyspec-platform; make the change there"
        frozen=1
        break
    done
done <"$changes"

exit "$frozen"
