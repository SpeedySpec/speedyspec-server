#!/usr/bin/env bash
# PHP's own costs for what the media port replaces, on the same test-kit files as crates/speedyspec-wp-media-isolate's
# benchmark: getimagesize() on the PNG and finfo_file() on each file, 1000 calls each, in µs per call.
#
#   scripts/bench-media-php.sh <php-binary>
set -euo pipefail

if [ "$#" -ne 1 ]; then
    echo "usage: scripts/bench-media-php.sh <php-binary>" >&2
    exit 2
fi
php="$1"
repo="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
dir="$(mktemp -d)"
trap 'rm -rf "$dir"' EXIT
(cd "$repo" && cargo run -q -p speedyspec-wp-media-testkit --example write_bench_fixtures "$dir")
"$php" -n -r '
$dir = $argv[1];
$time = function (callable $f) { $n = 1000; $t = hrtime(true); for ($i = 0; $i < $n; $i++) { $f(); } return (hrtime(true) - $t) / $n / 1000; };
printf("getimagesize png 640x480: %.1f us\n", $time(fn() => getimagesize("$dir/bench.png")));
$fi = finfo_open(FILEINFO_MIME_TYPE);
foreach (["bench.png", "bench.mp3", "bench.wav"] as $f) {
    printf("finfo_file %s: %.1f us\n", $f, $time(fn() => finfo_file($fi, "$dir/$f")));
}
' "$dir"
