<?php

declare(strict_types=1);

/**
 * Plugin Name: SpeedySpec SQL corpus logger
 * Description: Logs every query a plugin's install, cron and page views send, for the SpeedySpec SQL corpus.
 *
 * `collect-plugins.sh` installs it as a must-use plugin, beside one that defines `SPEEDYSPEC_CORPUS_SOURCE`.
 */

add_filter(
    'query',
    static function ($query)
    {
        $source = defined('SPEEDYSPEC_CORPUS_SOURCE') ? SPEEDYSPEC_CORPUS_SOURCE : 'plugin';
        $line = json_encode(['sql' => $query, 'source' => $source], JSON_UNESCAPED_SLASHES);
        if (false !== $line) {
            file_put_contents(WP_CONTENT_DIR . '/speedyspec-corpus.jsonl', $line . "\n", FILE_APPEND);
        }
        return $query;
    },
    PHP_INT_MAX
);
