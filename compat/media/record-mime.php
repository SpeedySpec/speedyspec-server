<?php
/**
 * Records WordPress's MIME lists and its filetype checks over the media corpora.
 *
 * Usage: php record-mime.php <wordpress-root> <out.json> <dir>...
 *
 * Run it with the static PHP the runtime embeds (`.php-build/buildroot/bin/php`), whose fileinfo answers the native
 * checks too. It records wp_get_mime_types(), wp_get_ext_types(), get_allowed_mime_types() (as ordered [key, value]
 * pairs, so the order survives JSON), wp_get_default_extension_for_mime_type() for every listed MIME type, and, for
 * every file in each directory under its own name and under the names in RENAMES, wp_check_filetype_and_ext(),
 * fileinfo's MIME type and wp_get_image_mime(). Files are named relative to the directory's parent, so the recording
 * names no machine's folders.
 */

declare(strict_types=1);

const RENAMES = ['jpg', 'png', 'mp3', 'mp4', 'mka', 'txt', 'pdf'];

[$_, $root, $out] = $argv + [null, null, null];
$dirs = array_slice($argv, 3);
if (!$root || !$out || !$dirs) {
    fwrite(STDERR, "usage: php record-mime.php <wordpress-root> <out.json> <dir>...\n");
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
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/shortcodes.php';
require ABSPATH . WPINC . '/media.php';

/** An array as ordered [key, value] pairs. */
function pairs(array $values): array
{
    $out = [];
    foreach ($values as $key => $value) {
        $out[] = [(string) $key, $value];
    }
    return $out;
}

$finfo = finfo_open(FILEINFO_MIME_TYPE);
$defaults = [];
foreach (array_unique(array_values(wp_get_mime_types())) as $mime) {
    $defaults[] = [$mime, wp_get_default_extension_for_mime_type($mime)];
}

$cases = [];
foreach ($dirs as $dir) {
    $dir = rtrim($dir, '/');
    $names = array_values(array_filter(scandir($dir), fn($n) => $n[0] !== '.' && !str_ends_with($n, '.md')));
    foreach ($names as $name) {
        $path = "$dir/$name";
        $stem = pathinfo($name, PATHINFO_FILENAME);
        $tried = [$name];
        foreach (RENAMES as $ext) {
            $tried[] = "$stem.$ext";
        }
        foreach (array_unique($tried) as $as) {
            $cases[] = [
                'file' => basename($dir) . "/$name",
                'name' => $as,
                'result' => wp_check_filetype_and_ext($path, $as),
                'finfo' => finfo_file($finfo, $path),
                'image_mime' => wp_get_image_mime($path),
            ];
        }
    }
}

$record = [
    'wordpress' => (function () use ($root) {
        include $root . WPINC . '/version.php';
        return $wp_version;
    })(),
    'php' => PHP_VERSION,
    'wp_get_mime_types' => pairs(wp_get_mime_types()),
    'wp_get_ext_types' => pairs(wp_get_ext_types()),
    'get_allowed_mime_types' => pairs(get_allowed_mime_types()),
    'default_extensions' => $defaults,
    'cases' => $cases,
];
$json = json_encode($record, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE);
if ($json === false) {
    fwrite(STDERR, 'json_encode: ' . json_last_error_msg() . "\n");
    exit(1);
}
file_put_contents($out, $json . "\n");
fwrite(STDERR, sprintf("recorded %d cases into %s\n", count($cases), $out));
