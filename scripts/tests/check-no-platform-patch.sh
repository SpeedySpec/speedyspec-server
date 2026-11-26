#!/usr/bin/env bash
# Tests scripts/check-no-platform-patch.sh against throwaway git repositories.
set -euo pipefail

script="$(cd "$(dirname "$0")/.." && pwd)/check-no-platform-patch.sh"
work="$(mktemp -d)"
trap 'rm -rf "$work"' EXIT

export GIT_AUTHOR_NAME=test GIT_AUTHOR_EMAIL=test@example.invalid
export GIT_COMMITTER_NAME=test GIT_COMMITTER_EMAIL=test@example.invalid

failures=0
fail() {
    echo "FAIL: $*" >&2
    failures=$((failures + 1))
}

# A repository with a workspace Cargo.toml and a .cargo/config.toml, neither patching anything, committed.
new_repo() {
    local repo="$work/$1"
    git init -q -b main "$repo"
    mkdir -p "$repo/.cargo"
    printf '%s\n' '[workspace]' 'members = ["crates/*"]' '' '[workspace.dependencies]' 'speedyspec-hooks = "0.2.0"' \
        >"$repo/Cargo.toml"
    printf '%s\n' '[env]' 'SPEEDYSPEC_COMPAT_DIR = { value = "compat", relative = true }' >"$repo/.cargo/config.toml"
    git -C "$repo" add -A
    git -C "$repo" commit -q -m base
    echo "$repo"
}

# expect <name> <exit status> <exact output or -> ; runs the check in the repository named <name>.
expect() {
    local name="$1" want_status="$2" want_output="$3" output status=0
    output="$(cd "$work/$name" && "$script" 2>&1)" || status=$?
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

new_repo clean >/dev/null
expect clean 0 -

# A committed patch of a platform crate in the workspace manifest.
repo="$(new_repo committed)"
printf '%s\n' '' '[patch.crates-io]' 'speedyspec-hooks = { path = "x" }' >>"$repo/Cargo.toml"
git -C "$repo" commit -qam patch
expect committed 1 'Cargo.toml: patches speedyspec-hooks'

# The same in .cargo/config.toml, committed, alongside a patch of a crate that is not a platform crate.
repo="$(new_repo committed-config)"
printf '%s\n' '' '[patch.crates-io]' 'pcre2-sys = { path = "vendor/pcre2-sys" }' 'speedyspec-php = { path = "x" }' \
    >>"$repo/.cargo/config.toml"
git -C "$repo" commit -qam patch
expect committed-config 1 '.cargo/config.toml: patches speedyspec-php'

# A patch table per crate, and a patch of another registry, are patches too.
repo="$(new_repo dotted)"
printf '%s\n' '' '[patch.crates-io.speedyspec-hooks]' 'path = "x"' '' '[patch."https://example.invalid/index"]' \
    '"speedyspec-cache" = { path = "y" }' >>"$repo/Cargo.toml"
git -C "$repo" commit -qam patch
expect dotted 1 "Cargo.toml: patches speedyspec-hooks
Cargo.toml: patches speedyspec-cache"

# Single-quoted keys and tables are TOML literal strings, and patches too.
repo="$(new_repo single-quoted)"
printf '%s\n' '' "[patch.'crates-io'.'speedyspec-hooks']" 'path = "x"' '' '[patch.crates-io]' \
    "'speedyspec-cache' = { path = \"y\" }" >>"$repo/Cargo.toml"
git -C "$repo" commit -qam patch
expect single-quoted 1 "Cargo.toml: patches speedyspec-hooks
Cargo.toml: patches speedyspec-cache"

# Appended to .cargo/config.toml but not committed (scripts/platform-local.sh in CI, or local work): only HEAD counts.
repo="$(new_repo uncommitted)"
printf '%s\n' '' '[patch.crates-io]' 'speedyspec-hooks = { path = "x" }' >>"$repo/.cargo/config.toml"
expect uncommitted 0 -

# Patching a crate that is not a platform crate is allowed.
repo="$(new_repo other-crate)"
printf '%s\n' '' '[patch.crates-io]' 'pcre2-sys = { path = "vendor/pcre2-sys" }' >>"$repo/Cargo.toml"
git -C "$repo" commit -qam patch
expect other-crate 0 -

# A platform crate named outside a patch table (a dependency) is not a patch.
repo="$(new_repo dependency)"
printf '%s\n' '' '[patch.crates-io]' 'pcre2-sys = { path = "vendor/pcre2-sys" }' '' '[workspace.lints.rust]' \
    'speedyspec-x = "allow"' >>"$repo/Cargo.toml"
git -C "$repo" commit -qam deps
expect dependency 0 -

# Neither file committed: nothing patches.
git init -q -b main "$work/empty"
git -C "$work/empty" commit -q --allow-empty -m empty
expect empty 0 -

if [ "$failures" -ne 0 ]; then
    echo "$failures failed" >&2
    exit 1
fi
echo "all passed"
