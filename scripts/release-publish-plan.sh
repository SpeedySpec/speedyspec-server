#!/usr/bin/env bash
# Says what publishing may do to a release, from its current assets (`gh release view <tag> --json assets`):
#   complete    every target's binary and signature, and SHA256SUMS, are there: change nothing. Builds are not
#               reproducible, so a new binary under a published URL would fail every server's checksum.
#   empty       none of them is there: upload the full set.
#   incomplete  some are: delete those and upload the full set. An incomplete release is never in an index, so no
#               server can have downloaded any of it.
# Assets with other names are ignored.
#
#   scripts/release-publish-plan.sh <binary> <assets.json>
set -euo pipefail

if [ "$#" -ne 2 ]; then
    echo "usage: scripts/release-publish-plan.sh <binary> <assets.json>" >&2
    exit 2
fi
binary="$1" assets="$2"
triples="x86_64-unknown-linux-musl aarch64-unknown-linux-musl aarch64-apple-darwin"

expected=(SHA256SUMS)
for triple in $triples; do
    expected+=("$binary-$triple" "$binary-$triple.minisig")
done

present=0
for name in "${expected[@]}"; do
    if jq -e --arg name "$name" 'any(.assets[]?; .name == $name)' "$assets" >/dev/null; then
        present=$((present + 1))
    fi
done

if [ "$present" -eq "${#expected[@]}" ]; then
    echo complete
elif [ "$present" -eq 0 ]; then
    echo empty
else
    echo incomplete
fi
