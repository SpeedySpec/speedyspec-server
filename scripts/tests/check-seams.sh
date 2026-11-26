#!/usr/bin/env bash
# Runs scripts/check-seams.sh against the fixture workspace and checks every rule's output.
set -uo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
fixture="$here/check-seams"
out=$("$here/../check-seams.sh" --manifest-path "$fixture/Cargo.toml" --allowlist "$fixture/allowlist.txt" 2>&1)
code=$?
fail=0
expect() {
    if ! grep -qxF -- "$1" <<<"$out"; then
        echo "missing: $1" >&2
        fail=1
    fi
}
refuse() {
    if grep -qF -- "$1" <<<"$out"; then
        echo "unexpected: $1" >&2
        fail=1
    fi
}
expect "seams: R1: no-meta has no [package.metadata.speedyspec] seam and role"
expect "seams: R2: b-impl -> a-impl (b/implementation -> a/implementation)"
expect "seams: R3: c-contracts -> a-impl (c/contracts -> a/implementation)"
expect "seams: R4: stale allowlist entry: b-impl -> c-gone"
refuse "b-impl -> a-contracts"
refuse "a-impl -> a-contracts"
refuse "seams: R2: b-other -> a-impl"
refuse "seams: R2: c-contracts -> a-impl"
if [[ $code -ne 1 ]]; then
    echo "exit code $code, expected 1" >&2
    fail=1
fi
# --write-allowlist writes today's violations and then the check passes R2/R3/R4.
tmp=$(mktemp)
"$here/../check-seams.sh" --manifest-path "$fixture/Cargo.toml" --allowlist "$tmp" --write-allowlist >/dev/null 2>&1
written=$(cat "$tmp")
for line in "b-impl -> a-impl" "b-other -> a-impl" "c-contracts -> a-impl"; do
    grep -qxF -- "$line" <<<"$written" || { echo "allowlist lacks: $line" >&2; fail=1; }
done
after=$("$here/../check-seams.sh" --manifest-path "$fixture/Cargo.toml" --allowlist "$tmp" 2>&1)
grep -qE "R2|R3|R4" <<<"$after" && { echo "violations after --write-allowlist: $after" >&2; fail=1; }
rm -f "$tmp"
if [[ $fail -ne 0 ]]; then
    echo "--- checker output ---" >&2
    echo "$out" >&2
    exit 1
fi
echo "check-seams fixture: ok"
