#!/bin/sh
# Collects the queries WordPress's PHPUnit suite sends (single site, then multisite) as JSON lines.
# Usage: collect-phpunit.sh <wordpress-develop dir> <out.jsonl>
set -eu
dir=$1
out=$2
here=$(cd "$(dirname "$0")" && pwd)
cp "$here/phpunit-bootstrap.php" "$dir/tests/phpunit/speedyspec-corpus-bootstrap.php"
trap 'rm -f "$dir/tests/phpunit/speedyspec-corpus-bootstrap.php"' EXIT
cd "$dir"
rm -f speedyspec-corpus.jsonl
npm run test:php -- --bootstrap tests/phpunit/speedyspec-corpus-bootstrap.php || true
npm run test:php -- --bootstrap tests/phpunit/speedyspec-corpus-bootstrap.php -c tests/phpunit/multisite.xml || true
mv speedyspec-corpus.jsonl "$out"
