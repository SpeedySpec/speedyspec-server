<?php
/**
 * Records WordPress's image functions and PHP's own getimagesize() over the image corpus.
 *
 * Usage: php record-images.php <wordpress-root> <images-dir> <out.json>
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`): the fast path of the native
 * wp_getimagesize() is this PHP's getimagesize(), so what it answers here decides which image types take it.
 * Binary strings (JPEG APPn segments in $image_info) are recorded as length and SHA-256.
 */

declare(strict_types=1);

[$_, $root, $images, $out] = $argv + [null, null, null, null];
if (!$root || !$images || !$out) {
    fwrite(STDERR, "usage: php record-images.php <wordpress-root> <images-dir> <out.json>\n");
    exit(2);
}
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
error_reporting(E_ALL & ~E_DEPRECATED & ~E_WARNING);

require ABSPATH . WPINC . '/plugin.php';
add_filter('pre_option', function ($pre, $option, $default) {
    return ['blog_charset' => 'UTF-8'][$option] ?? ($default === false ? '' : $default);
}, 10, 3);
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/class-wp-error.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/shortcodes.php';
require ABSPATH . WPINC . '/media.php';

/** As record.php: binary strings by length and hash, NAN and INF as strings. */
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
        return ['$bytes' => ['len' => strlen($value), 'sha256' => hash('sha256', $value)]];
    }
    if (is_float($value) && (is_nan($value) || is_infinite($value))) {
        return (string) $value;
    }
    return $value;
}

$files = [];
foreach (array_filter(scandir($images), fn($n) => $n[0] !== '.') as $name) {
    $path = rtrim($images, '/') . '/' . $name;
    $info = null;
    $size = @getimagesize($path, $info);
    $wp_info = null;
    $wp_size = wp_getimagesize($path, $wp_info);
    $times = [];
    for ($i = 0; $i < 5; $i++) {
        $t = hrtime(true);
        wp_getimagesize($path);
        $times[] = (hrtime(true) - $t) / 1000;
    }
    sort($times);
    $files[$name] = [
        'getimagesize' => plain($size),
        'image_info' => plain($info),
        'exif_imagetype' => @exif_imagetype($path),
        'wp_getimagesize' => plain($wp_size),
        'wp_getimagesize_info' => plain($wp_info),
        'wp_get_image_mime' => wp_get_image_mime($path),
        'wp_get_webp_info' => wp_get_webp_info($path),
        'wp_get_avif_info' => wp_get_avif_info($path),
        'us' => (int) round($times[2]),
    ];
}

$record = [
    'wordpress' => (function () use ($root) {
        include $root . WPINC . '/version.php';
        return $wp_version;
    })(),
    'php' => PHP_VERSION,
    'imagick' => extension_loaded('imagick') ? \Imagick::getVersion()['versionString'] : null,
    'files' => $files,
];
file_put_contents($out, json_encode($record, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_PRESERVE_ZERO_FRACTION) . "\n");
fwrite(STDERR, sprintf("recorded %d images into %s\n", count($files), $out));
