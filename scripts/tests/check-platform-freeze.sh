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
    # The comment lines would match the `comments` case's files if they were read as entries.
    printf '%s\n' '#crates/' '#notes' '' 'crates/speedyspec-hooks/' 'scripts/build-php.sh' >"$repo/scripts/platform-paths.txt"
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

# Comment lines are skipped: `#crates/` would match a file under a directory named `#crates`, and `#notes` a file
# named `#notes`, if they were entries.
repo="$(new_repo comments)"
mkdir -p "$repo/#crates"
echo 'x' >"$repo/#crates/lib.rs"
echo 'notes' >"$repo/#notes"
git -C "$repo" add -A
git -C "$repo" commit -qm change
expect comments 0 -

# A path git would C-quote (non-ASCII) is still matched, and printed as it is.
repo="$(new_repo non-ascii)"
echo 'x' >"$repo/crates/speedyspec-hooks/src/ünïcode.rs"
git -C "$repo" add -A
git -C "$repo" commit -qm change
expect non-ascii 1 'crates/speedyspec-hooks/src/ünïcode.rs: moved to speedyspec-platform; make the change there'

# The list is read from the base: dropping an entry in the same change does not unfreeze its path.
repo="$(new_repo list-from-base)"
grep -v '^scripts/build-php.sh$' "$repo/scripts/platform-paths.txt" >"$repo/list.tmp"
mv "$repo/list.tmp" "$repo/scripts/platform-paths.txt"
echo 'echo again' >>"$repo/scripts/build-php.sh"
git -C "$repo" commit -qam change
expect list-from-base 1 'scripts/build-php.sh: moved to speedyspec-platform; make the change there'

# A base with no list is an error, not a pass.
repo="$(new_repo no-list-at-base)"
git -C "$repo" checkout -q base
git -C "$repo" rm -q scripts/platform-paths.txt
git -C "$repo" commit -qm 'no list'
git -C "$repo" checkout -q change
expect no-list-at-base 2 'base has no scripts/platform-paths.txt (or base is not a commit)'

if [ "$failures" -ne 0 ]; then
    echo "$failures failing case(s)" >&2
    exit 1
fi
echo "PASS"
