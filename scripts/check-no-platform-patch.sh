#!/usr/bin/env bash
# Fails when the committed Cargo.toml or .cargo/config.toml patches a platform crate: a `speedyspec-*` key under a
# `[patch.*]` table, or a `[patch.<registry>.speedyspec-*]` table. The platform crates come from crates.io; a patch
# pointing them at a speedyspec-platform checkout (scripts/platform-local.sh) is for local work and CI proofs only,
# and is never committed.
#
#   scripts/check-no-platform-patch.sh
#
# Reads the files at HEAD in the repository holding the current directory, so an uncommitted patch passes. Prints
# `<file>: patches <crate>` for each one and exits 1, or exits 0 when there is none.
set -euo pipefail

# Table headers and keys that could name a platform crate, as `HEAD:<file>:<line>`.
lines="$(git grep -E '^[[:space:]]*(\[|"?speedyspec-)' HEAD -- Cargo.toml .cargo/config.toml || true)"

found="$(printf '%s\n' "$lines" | awk '
    $0 == "" { next }
    {
        rest = substr($0, length("HEAD:") + 1)
        file = substr(rest, 1, index(rest, ":") - 1)
        line = substr(rest, length(file) + 2)
        sub(/^[[:space:]]+/, "", line)
        if (file != current) { current = file; patch = 0 }
    }
    line ~ /^\[/ {
        header = line
        sub(/\].*/, "", header)
        sub(/^\[+/, "", header)
        gsub(/[[:space:]"]/, "", header)
        patch = (header ~ /^patch\./)
        # [patch.<registry>.speedyspec-x]: the table is the patch.
        if (patch && match(header, /\.speedyspec-[A-Za-z0-9_-]+$/)) {
            print file ": patches " substr(header, RSTART + 1)
            patch = 0
        }
        next
    }
    patch {
        key = line
        sub(/[[:space:]]*[=.].*/, "", key)
        gsub(/"/, "", key)
        if (key ~ /^speedyspec-/) print file ": patches " key
    }
')"

if [ -n "$found" ]; then
    printf '%s\n' "$found"
    exit 1
fi
