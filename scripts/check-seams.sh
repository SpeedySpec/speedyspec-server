#!/usr/bin/env bash
# Seams meet only through the Container: a seam's crates may depend on another seam's contracts and on the platform,
# never on its implementations. Every crate declares its seam and role in [package.metadata.speedyspec].
#
#   R1  every workspace member declares seam and role
#   R2  a member (not root or test) has no normal dependency on another seam's member unless that member's role is
#       contracts or platform
#   R3  a contracts member depends only on contracts and platform members, and its normal dependency tree holds no
#       PHP (speedyspec-php, speedyspec-php-ext, ext-php-rs)
#   R4  every allowlist line is still a real violation, so the allowlist only shrinks
#
# R2 and R3 edges listed in the allowlist (one "from -> to" per line) are tolerated while the migration runs.
#
# Crates whose directory is listed in scripts/platform-paths.txt moved to speedyspec-platform, which checks their seam
# and role. They are out of scope here: no rule applies to them, and an edge to or from one is never a violation.
#
# Usage: scripts/check-seams.sh [--manifest-path Cargo.toml] [--allowlist file] [--platform-paths file]
#                               [--write-allowlist]
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
manifest="$root/Cargo.toml"
allowlist="$root/scripts/seam-allowlist.txt"
platform_paths="$root/scripts/platform-paths.txt"
write=0
while [[ $# -gt 0 ]]; do
    case "$1" in
        --manifest-path) manifest="$2"; shift 2 ;;
        --allowlist) allowlist="$2"; shift 2 ;;
        --platform-paths) platform_paths="$2"; shift 2 ;;
        --write-allowlist) write=1; shift ;;
        *) echo "check-seams: unknown argument $1" >&2; exit 2 ;;
    esac
done

metadata=$(cargo metadata --format-version 1 --no-deps --manifest-path "$manifest")

# The crate directories (relative to the workspace root, without the trailing /) that moved to speedyspec-platform.
moved="[]"
if [[ -f $platform_paths ]]; then
    moved=$(grep -E '^crates/[^/]+/$' "$platform_paths" | sed 's#/$##' | jq -R . | jq -s .)
fi
# Drop the moved crates, so that no rule sees them, either as members or as edge endpoints.
metadata=$(jq --argjson moved "$moved" '
    .workspace_root as $root
    | .packages |= map(select((.manifest_path | ltrimstr($root + "/") | rtrimstr("/Cargo.toml")) as $dir
        | ($moved | index($dir)) == null))
' <<<"$metadata")

# One line per member: name seam role (seam and role "-" when missing).
members=$(jq -r '.packages[] | [.name, (.metadata.speedyspec.seam // "-"), (.metadata.speedyspec.role // "-")] | @tsv' <<<"$metadata")

failures=()
while IFS=$'\t' read -r name s r; do
    [[ -z $name ]] && continue
    if [[ $s == "-" || $r == "-" ]]; then
        failures+=("seams: R1: $name has no [package.metadata.speedyspec] seam and role")
    fi
done <<<"$members"

# One line per R2/R3 violation among normal dependencies between members: rule, edge, detail.
violations=$(jq -r '
    (reduce .packages[] as $p ({}; .[$p.name] = {seam: ($p.metadata.speedyspec.seam // null), role: ($p.metadata.speedyspec.role // null)})) as $m
    | .packages[] | .name as $from | $m[$from] as $f
    | select($f.role != null and $f.seam != null)
    | [.dependencies[] | select(.kind == null) | .name | select($m[.] != null)] | unique[] as $to
    | $m[$to] as $t
    | select($t.role != null and $t.seam != null)
    | "\($from) -> \($to)" as $edge
    | "\($edge) (\($f.seam)/\($f.role) -> \($t.seam)/\($t.role))" as $detail
    | if $f.role == "contracts" then
        (if ($t.role == "contracts" or $t.role == "platform") then empty else ["R3", $edge, $detail] end)
      elif ($f.role == "root" or $f.role == "test") then empty
      elif ($f.seam != $t.seam and $t.role != "contracts" and $t.role != "platform") then ["R2", $edge, $detail]
      else empty end
    | @tsv
' <<<"$metadata" | sort -u)

allowed=""
if [[ -f $allowlist ]]; then
    allowed=$(grep -v -e '^[[:space:]]*#' -e '^[[:space:]]*$' "$allowlist" || true)
fi

# R3: no PHP anywhere under a contracts crate.
while IFS=$'\t' read -r name s r; do
    [[ $r == "contracts" ]] || continue
    tree=$(cargo tree --manifest-path "$manifest" -p "$name" -e normal --prefix none 2>/dev/null | awk '{print $1}' | sort -u)
    for php in speedyspec-php speedyspec-php-ext ext-php-rs; do
        if grep -qx "$php" <<<"$tree"; then
            failures+=("seams: R3: $name depends on $php (contracts must hold no PHP)")
        fi
    done
done <<<"$members"

if [[ $write -eq 1 ]]; then
    {
        echo "# Cross-seam edges tolerated while the seam separation runs; each seam's PR removes its own lines."
        [[ -n $violations ]] && cut -f2 <<<"$violations" | sort -u
    } >"$allowlist"
    echo "check-seams: wrote $(grep -vc '^#' "$allowlist") edges to $allowlist"
    exit 0
fi

current=""
while IFS=$'\t' read -r rule edge detail; do
    [[ -z $rule ]] && continue
    current+="$edge"$'\n'
    if ! grep -qxF -- "$edge" <<<"$allowed"; then
        failures+=("seams: $rule: $detail")
    fi
done <<<"$violations"
while IFS= read -r line; do
    [[ -z $line ]] && continue
    if ! grep -qxF -- "$line" <<<"$current"; then
        failures+=("seams: R4: stale allowlist entry: $line")
    fi
done <<<"$allowed"

if [[ ${#failures[@]} -gt 0 ]]; then
    printf '%s\n' "${failures[@]}"
    exit 1
fi
echo "check-seams: every seam meets the others through contracts ($(grep -c . <<<"$allowed" || true) allowlisted edges left)"
