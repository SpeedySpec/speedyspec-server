#!/bin/sh
# Collects the queries popular plugins send while they install, run cron and serve pages, as JSON lines.
# Usage: collect-plugins.sh <wordpress-develop dir> <out.jsonl>
set -eu
dir=$1
out=$2
here=$(cd "$(dirname "$0")" && pwd)
plugins="$here/../../compat/sql-corpus/plugins.json"
cd "$dir"
: > "$out"
php -r 'foreach (json_decode(file_get_contents($argv[1]), true) as $p) { echo $p["slug"], " ", $p["version"], "\n"; }' "$plugins" |
while read -r slug version; do
    npm run env:reset
    npm run env:install
    mkdir -p src/wp-content/mu-plugins
    cp "$here/corpus-logger.php" src/wp-content/mu-plugins/speedyspec-corpus-logger.php
    printf "<?php\n\ndeclare(strict_types=1);\n\ndefine('SPEEDYSPEC_CORPUS_SOURCE', 'plugin:%s@%s');\n" "$slug" "$version" \
        > src/wp-content/mu-plugins/speedyspec-corpus-source.php
    npm run env:cli -- plugin install "$slug" --version="$version" --activate
    npm run env:cli -- cron event run --all || true
    for path in / '/?s=corpus' '/?p=1' /feed/ /wp-json/ /wp-login.php; do
        curl -s -o /dev/null "http://localhost:8889$path" || true
    done
    npm run env:cli -- plugin deactivate "$slug" --uninstall || true
    rm -f src/wp-content/mu-plugins/speedyspec-corpus-logger.php src/wp-content/mu-plugins/speedyspec-corpus-source.php
    cat src/wp-content/speedyspec-corpus.jsonl >> "$out" 2>/dev/null || true
    rm -f src/wp-content/speedyspec-corpus.jsonl
done
