#!/usr/bin/env bash
# The server binary must not depend on the embedded PHP: it only talks FastCGI to the runtime's workers.
set -euo pipefail
cd "$(dirname "$0")/.."
deps=$(cargo tree -p speedyspec-server -e normal --prefix none --all-features | awk '{print $1}' | sort -u)
for crate in speedyspec-php speedyspec-php-ext ext-php-rs speedyspec-wp-hooks-php speedyspec-container-php; do
    if grep -qx "$crate" <<<"$deps"; then
        echo "speedyspec-server depends on $crate" >&2
        exit 1
    fi
done
echo "no PHP in speedyspec-server"
