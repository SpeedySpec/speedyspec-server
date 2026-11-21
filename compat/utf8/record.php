<?php
/**
 * Records WordPress's UTF-8 functions into cases.json, which the native port is replayed against.
 *
 * Usage: .php-build/buildroot/bin/php compat/utf8/record.php <wordpress-root>
 *
 * Run it with the static PHP the port embeds, so mbstring and PCRE are the same build.
 */

[$_, $root] = $argv + [null, null];
if (!$root) {
    fwrite(STDERR, "usage: php record.php <wordpress-root>\n");
    exit(2);
}
$root = rtrim($root, '/') . '/';
error_reporting(E_ALL & ~E_DEPRECATED);
function _deprecated_function($function, $version, $replacement = '')
{
    $GLOBALS['speedyspec_deprecated'][] = array($function, $version, $replacement);
}
require $root . 'wp-includes/compat.php';
require $root . 'wp-includes/compat-utf8.php';
require $root . 'wp-includes/utf8.php';
require __DIR__ . '/calls.php';

$cases = array();
foreach (speedyspec_utf8_inputs(__DIR__ . '/../../crates/speedyspec-wp-utf8/tests/data/utf8-cases.txt') as $s) {
    $cases[] = array('input' => base64_encode($s), 'calls' => speedyspec_utf8_calls($s));
}
$version = null;
require $root . 'wp-includes/version.php';
file_put_contents(
    __DIR__ . '/cases.json',
    json_encode(array('wordpress' => $wp_version, 'php' => PHP_VERSION, 'cases' => $cases), JSON_UNESCAPED_SLASHES) . "\n"
);
echo count($cases), " cases recorded against WordPress $wp_version on PHP ", PHP_VERSION, "\n";
