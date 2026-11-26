#!/usr/bin/env bash
# Prints the PHP version the server embeds: the php-version file of the speedyspec-php crate Cargo resolved (from
# crates.io, or from a speedyspec-platform checkout patched in with scripts/platform-local.sh). scripts/build-php.sh
# builds that version, and CI keys its PHP caches on it. Needs cargo and jq.
#
#   scripts/php-version.sh        # 8.5.12
set -euo pipefail

repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
# The manifest of the speedyspec-php Cargo resolved; its php-version file sits beside it.
manifest="$(cargo metadata --format-version 1 --manifest-path "$repo/Cargo.toml" | jq -r '.packages[] | select(.name=="speedyspec-php") | .manifest_path')"
if [ -z "$manifest" ]; then
    echo "php-version.sh: speedyspec-php is not in the dependency graph" >&2
    exit 1
fi
# Two versions of speedyspec-php in the graph would mean two PHP pins; refuse rather than pick one.
if [ "$(printf '%s\n' "$manifest" | wc -l)" -ne 1 ]; then
    echo "php-version.sh: more than one speedyspec-php in the dependency graph:" >&2
    printf '%s\n' "$manifest" >&2
    exit 1
fi
php_version="$(tr -d '[:space:]' < "$(dirname "$manifest")/php-version")"
if [ -z "$php_version" ]; then
    echo "php-version.sh: speedyspec-php's php-version is empty" >&2
    exit 1
fi
echo "$php_version"
