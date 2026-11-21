#!/usr/bin/env bash
# Names the workspace crates a component is built from: the component and every workspace crate it reaches through
# normal and build dependencies (not dev-dependencies), one per line, sorted. A release tests these crates only, so a
# failing test in a crate the component does not contain cannot hold its release back; CI tests the whole workspace.
#
#   cargo metadata --format-version 1 --no-deps > metadata.json
#   scripts/release-test-packages.sh <component> metadata.json
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: scripts/release-test-packages.sh <component> <metadata.json>" >&2
    exit 2
fi
component="$1" metadata="$2"

if ! jq -e --arg c "$component" 'any(.packages[]; .name == $c)' "$metadata" >/dev/null; then
    echo "$component is not a crate in this workspace" >&2
    exit 1
fi

jq -r --arg c "$component" '
    (.packages | map(.name)) as $names
    | (.packages | map({key: .name, value: [
        .dependencies[]
        | select(.path != null and (.kind == null or .kind == "build"))
        | .name
        | select(. as $d | $names | index($d))
      ]}) | from_entries) as $deps
    | def reach($seen; $todo):
        if ($todo | length) == 0 then $seen
        else ($todo[0]) as $next
            | if ($seen | index($next)) then reach($seen; $todo[1:])
              else reach($seen + [$next]; $todo[1:] + $deps[$next])
              end
        end;
      reach([]; [$c]) | sort | .[]
' "$metadata"
