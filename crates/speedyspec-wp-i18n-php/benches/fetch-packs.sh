#!/bin/sh
# Downloads the language packs the request benchmark reads (core de_DE for WordPress 6.9, WooCommerce 11.3.1
# de_DE) from WordPress.org into benches/packs/. They are not committed.
set -eu
dir="$(cd "$(dirname "$0")" && pwd)/packs"
mkdir -p "$dir/core" "$dir/woo"
tmp="$(mktemp -d)"
trap 'rm -rf "$tmp"' EXIT
curl -sSfo "$tmp/core.zip" https://downloads.wordpress.org/translation/core/6.9/de_DE.zip
curl -sSfo "$tmp/woo.zip" https://downloads.wordpress.org/translation/plugin/woocommerce/11.3.1/de_DE.zip
unzip -oq "$tmp/core.zip" -d "$dir/core"
unzip -oq "$tmp/woo.zip" -d "$dir/woo"
echo "packs in $dir"
