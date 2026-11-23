<?php
/**
 * Records WordPress's audio and video metadata functions, and the getID3 data behind them, over the media corpus.
 *
 * Usage: php record.php <wordpress-root> <corpus-dir> <out.json>
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`). For every corpus file it calls
 * wp_read_audio_metadata() or wp_read_video_metadata() (by extension, as WordPress picks by MIME type), captures the
 * filter's four arguments, calls wp_get_media_creation_timestamp() on the raw data, and times five calls. Binary
 * strings are recorded as their length and SHA-256; short non-UTF-8 strings as hex, so the fixture is plain JSON.
 */

declare(strict_types=1);

[$_, $root, $corpus, $out] = $argv + [null, null, null, null];
if (!$root || !$corpus || !$out) {
    fwrite(STDERR, "usage: php record.php <wordpress-root> <corpus-dir> <out.json>\n");
    exit(2);
}
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
error_reporting(E_ALL & ~E_DEPRECATED);

require ABSPATH . WPINC . '/plugin.php';
// What a default install's options answer; no database here. The media functions reach options only through kses.
add_filter('pre_option', function ($pre, $option, $default) {
    return ['blog_charset' => 'UTF-8', 'html_type' => 'text/html'][$option] ?? ($default === false ? '' : $default);
}, 10, 3);
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/kses.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . 'wp-admin/includes/media.php';

$audio_ext = [
    'mp3', 'm4a', 'flac', 'ogg', 'opus', 'wav', 'aiff', 'wma', 'ra', 'ac3', 'dts', 'aac', 'mka', 'mid',
    'eac3', 'caf', 'wv', 'tta', 'au', 'mp2', 'w64', 'voc',
];

/** A value as plain JSON: binary strings by length and hash, short non-UTF-8 strings as hex, NAN and INF as strings. */
function plain($value)
{
    if (is_array($value)) {
        $out = [];
        foreach ($value as $k => $v) {
            $out[$k] = plain($v);
        }
        return $out;
    }
    if (is_string($value) && !mb_check_encoding($value, 'UTF-8')) {
        if (strlen($value) <= 256 && strpos($value, "\0") === false) {
            return ['$latin1' => bin2hex($value)];
        }
        return ['$bytes' => ['len' => strlen($value), 'sha256' => hash('sha256', $value)]];
    }
    if (is_float($value) && (is_nan($value) || is_infinite($value))) {
        return (string) $value;
    }
    return $value;
}

$captured = null;
foreach (['wp_read_audio_metadata', 'wp_read_video_metadata'] as $hook) {
    add_filter($hook, function ($metadata, $file, $file_format, $data) use (&$captured) {
        $captured = ['file_format' => $file_format, 'data' => $data];
        return $metadata;
    }, PHP_INT_MAX, 4);
}

$files = [];
$names = array_values(array_filter(scandir($corpus), fn($n) => $n[0] !== '.' && !str_ends_with($n, '.md')));
foreach ($names as $name) {
    $path = rtrim($corpus, '/') . '/' . $name;
    $kind = in_array(strtolower(pathinfo($name, PATHINFO_EXTENSION)), $audio_ext, true) ? 'audio' : 'video';
    $fn = $kind === 'audio' ? 'wp_read_audio_metadata' : 'wp_read_video_metadata';
    $captured = null;
    $metadata = $fn($path);
    $first = $captured;
    $times = [];
    for ($i = 0; $i < 5; $i++) {
        $t = hrtime(true);
        $fn($path);
        $times[] = (hrtime(true) - $t) / 1000;
    }
    sort($times);
    // getID3 records the absolute path it read; the corpus directory becomes `<corpus>` so the recording names no
    // machine's folders.
    $data = $first['data'] ?? null;
    foreach (['filepath', 'filenamepath'] as $key) {
        if (is_array($data) && isset($data[$key]) && is_string($data[$key])) {
            $data[$key] = str_replace(rtrim($corpus, '/'), '<corpus>', $data[$key]);
        }
    }
    $files[$name] = [
        'kind' => $kind,
        'metadata' => plain($metadata),
        'file_format' => plain($first['file_format'] ?? null),
        'data' => plain($data),
        'creation_timestamp' => $first ? wp_get_media_creation_timestamp($first['data']) : null,
        'us' => (int) round($times[2]),
    ];
}

require_once ABSPATH . WPINC . '/ID3/getid3.php';
$record = [
    'wordpress' => (function () use ($root) {
        include $root . WPINC . '/version.php';
        return $wp_version;
    })(),
    'getid3' => getID3::VERSION,
    'php' => PHP_VERSION,
    'files' => $files,
];
$json = json_encode($record, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION);
if ($json === false) {
    fwrite(STDERR, 'json_encode: ' . json_last_error_msg() . "\n");
    exit(1);
}
file_put_contents($out, $json . "\n");
fwrite(STDERR, sprintf("recorded %d files into %s\n", count($files), $out));
