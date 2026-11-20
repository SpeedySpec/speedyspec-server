<?php

declare(strict_types=1);

/**
 * Logs every query WordPress's PHPUnit suite sends, for the SpeedySpec SQL corpus.
 *
 * Copied into `tests/phpunit/` by `collect-phpunit.sh`, it registers the logger before WordPress's own test bootstrap
 * loads WordPress, so the queries of the install and of every test are seen.
 */

require_once __DIR__ . '/includes/functions.php';

tests_add_filter(
    'query',
    static function ($query)
    {
        // The same text is logged once per process; the corpus counts shapes, not repetitions.
        static $seen = [];
        $key = md5((string) $query);
        if (isset($seen[$key])) {
            return $query;
        }
        $seen[$key] = true;
        $line = json_encode(['sql' => $query, 'source' => 'wordpress-phpunit'], JSON_UNESCAPED_SLASHES);
        if (false !== $line) {
            file_put_contents(dirname(__DIR__, 2) . '/speedyspec-corpus.jsonl', $line . "\n", FILE_APPEND);
        }
        return $query;
    },
    PHP_INT_MAX
);

require __DIR__ . '/includes/bootstrap.php';
