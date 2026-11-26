#!/usr/bin/env bash
# Tests scripts/check-platform-freeze.sh against throwaway git repositories.
set -euo pipefail

script="$(cd "$(dirname "$0")/.." && pwd)/check-platform-freeze.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# A repository with a two-entry paths file (plus a comment and a blank line), committed as `base`, then checked out
# on a `change` branch.
new_repo() {
    local repo="$work/$1"
    git init -q -b base "$repo"
    mkdir -p "$repo/scripts" "$repo/crates/speedyspec-hooks/src" "$repo/crates/speedyspec-hooks-memory/src"
    printf '%s\n' '# The frozen paths.' '' 'crates/speedyspec-hooks/' 'scripts/build-php.sh' >"$repo/scripts/platform-paths.txt"
    echo 'pub fn a() {}' >"$repo/crates/speedyspec-hooks/src/lib.rs"
    echo 'pub fn b() {}' >"$repo/crates/speedyspec-hooks-memory/src/lib.rs"
    echo 'echo build' >"$repo/scripts/build-php.sh"
    git -C "$repo" add -A
    git -C "$repo" commit -q -m base
    git -C "$repo" checkout -q -b change
    echo "$repo"
}

# expect <name> <exit status> <exact output or -> ; runs the check in the repository named <name>.
expect() {
    local name="$1" want_status="$2" want_output="$3" output status=0
    output="$(cd "$work/$name" && "$script" base 2>&1)" || status=$?
    if [ "$status" != "$want_status" ]; then
        fail "$name: exit $status, want $want_status (output: $output)"
        return
    fi
    if [ "$want_output" != "-" ] && [ "$output" != "$want_output" ]; then
        fail "$name: output '$output', want '$want_output'"
        return
    fi
    echo "ok: $name"
}

repo="$(new_repo frozen-file)"
echo 'pub fn a2() {}' >>"$repo/crates/speedyspec-hooks/src/lib.rs"
git -C "$repo" commit -qam change
expect frozen-file 1 'crates/speedyspec-hooks/src/lib.rs: moved to speedyspec-platform; make the change there'

repo="$(new_repo prefix-boundary)"
echo 'pub fn b2() {}' >>"$repo/crates/speedyspec-hooks-memory/src/lib.rs"
git -C "$repo" commit -qam change
expect prefix-boundary 0 -

repo="$(new_repo exact-file)"
echo 'echo again' >>"$repo/scripts/build-php.sh"
git -C "$repo" commit -qam change
expect exact-file 1 'scripts/build-php.sh: moved to speedyspec-platform; make the change there'

repo="$(new_repo exact-only)"
echo 'echo backup' >"$repo/scripts/build-php.sh.bak"
git -C "$repo" add -A
git -C "$repo" commit -qm change
expect exact-only 0 -

repo="$(new_repo deleted)"
git -C "$repo" rm -q crates/speedyspec-hooks/src/lib.rs
git -C "$repo" commit -qm change
expect deleted 1 'crates/speedyspec-hooks/src/lib.rs: moved to speedyspec-platform; make the change there'

repo="$(new_repo renamed-out)"
mkdir -p "$repo/crates/elsewhere/src"
git -C "$repo" mv crates/speedyspec-hooks/src/lib.rs crates/elsewhere/src/lib.rs
git -C "$repo" commit -qm change
expect renamed-out 1 'crates/speedyspec-hooks/src/lib.rs: moved to speedyspec-platform; make the change there'

# Comments and blank lines: a change to a file named like the comment, or at the root, is not frozen.
repo="$(new_repo comments)"
echo 'notes' >"$repo/README.md"
git -C "$repo" add -A
git -C "$repo" commit -qm change
expect comments 0 -

if [ "$failures" -ne 0 ]; then
    echo "$failures failing case(s)" >&2
    exit 1
fi
echo "PASS"
