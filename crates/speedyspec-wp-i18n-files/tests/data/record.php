<?php
/**
 * Records what WordPress's own WP_Translation_File readers make of every fixture here: entries, headers, error.
 *
 * Usage: .php-build/buildroot/bin/php record.php <wordpress-root>
 */

[$_, $root] = $argv + [null, null];
$root = rtrim($root, '/') . '/wp-includes/';
require $root . 'pomo/plural-forms.php';
require $root . 'l10n/class-wp-translation-file.php';
require $root . 'l10n/class-wp-translation-file-mo.php';
require $root . 'l10n/class-wp-translation-file-php.php';
error_reporting(0);

$b64 = function ($a) {
    $out = array();
    foreach ($a as $k => $v) {
        $out[] = array(base64_encode((string) $k), is_string($v) ? base64_encode($v) : array('php' => var_export($v, true)));
    }
    return $out;
};
$out = array();
foreach (glob(__DIR__ . '/*.{mo,php}', GLOB_BRACE) as $file) {
    if (basename($file) === 'record.php') {
        continue;
    }
    $f = WP_Translation_File::create($file);
    if (false === $f) {
        $out[basename($file)] = array('create' => false);
        continue;
    }
    $entries = $f->entries();
    $out[basename($file)] = array('entries' => $b64($entries), 'headers' => $b64($f->headers()), 'error' => $f->error());
}
file_put_contents(__DIR__ . '/recorded.json', json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n");
echo count($out), " files recorded\n";
