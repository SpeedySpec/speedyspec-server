#!/usr/bin/env bash
# Usage: scripts/check-platform-freeze.sh <base-ref>
#
# The paths in scripts/platform-paths.txt moved to SpeedySpec/speedyspec-platform. Until the server depends on the
# published crates, they must not change here. Prints every path that changed between <base-ref> and HEAD under a
# frozen entry, and exits 1 if there is any. An entry ending in `/` matches as a directory prefix; any other entry
# matches one file exactly. Renames count as a deletion plus an addition, so moving a file out of a frozen directory
# is a change too.
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: $0 <base-ref>" >&2
    exit 2
fi
base="$1"
root="$(git rev-parse --show-toplevel)"
paths_file="$root/scripts/platform-paths.txt"
if [ ! -f "$paths_file" ]; then
    echo "$paths_file: not found" >&2
    exit 2
fi

entries=()
while IFS= read -r line || [ -n "$line" ]; do
    case "$line" in
    '' | '#'*) continue ;;
    esac
    entries+=("$line")
done <"$paths_file"

# Fails the script (set -e) on an unknown base ref, rather than passing with no changes.
changes="$(git -C "$root" diff --name-only --no-renames "$base"...HEAD)"

frozen=0
while IFS= read -r changed; do
    [ -n "$changed" ] || continue
    for entry in "${entries[@]}"; do
        if [ "${entry%/}" != "$entry" ]; then
            [ "${changed#"$entry"}" != "$changed" ] || continue
        else
            [ "$changed" = "$entry" ] || continue
        fi
        echo "$changed: moved to speedyspec-platform; make the change there"
        frozen=1
        break
    done
done <<<"$changes"

exit "$frozen"
