<?php
/**
 * Records WordPress's MO reading and the MO/.l10n.php writers into mo-recorded.json, for the Rust port's tests: for
 * each fixture, what MO::import_from_reader() hands to make_headers() and make_entry() (and its result); the
 * strings MO::export() packs and the bytes it writes; and WP_Translation_File's headers, entries and transform()
 * output in both formats. Byte strings are hex.
 *
 * Usage: .php-build/buildroot/bin/php record-mo.php <wordpress-root>
 */

[$_, $root] = $argv + [null, null];
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
function __($t) { return $t; }
require $root . 'wp-includes/pomo/mo.php';
foreach (array('class-wp-translation-controller.php', 'class-wp-translations.php', 'class-wp-translation-file.php',
    'class-wp-translation-file-mo.php', 'class-wp-translation-file-php.php') as $f) {
    require $root . 'wp-includes/l10n/' . $f;
}

class SpeedySpec_Recording_MO extends MO {
    public $events = array();
    public function make_headers($translation) {
        $this->events[] = array('header', bin2hex($translation));
        return parent::make_headers($translation);
    }
    public function &make_entry($original, $translation) {
        $this->events[] = array('pair', bin2hex($original), bin2hex($translation));
        $entry = &parent::make_entry($original, $translation);
        return $entry;
    }
}

/**
 * A list as written, or `['md5' => …]` of its canonical form (items joined by "\n", each item's parts by ':') when it
 * is long, so the recording stays small.
 */
function speedyspec_small(array $items) {
    if (count($items) <= 40) {
        return $items;
    }
    return array('md5' => md5(implode("\n", array_map(function ($item) {
        return implode(':', array_map(function ($p) { return is_bool($p) ? ($p ? '1' : '0') : (string) $p; }, $item));
    }, $items))));
}

/** Bytes as hex, or `['md5' => md5( bytes )]` when long. */
function speedyspec_bytes($b) {
    if (false === $b) {
        return false;
    }
    return strlen($b) <= 4000 ? bin2hex($b) : array('md5' => md5($b));
}

function speedyspec_hex_pairs(array $a): array {
    $out = array();
    foreach ($a as $k => $v) {
        $out[] = array(is_int($k) ? $k : bin2hex((string) $k), is_int($k), bin2hex((string) $v));
    }
    return $out;
}

$files = array_merge(glob(__DIR__ . '/*.mo'), glob(__DIR__ . '/mo/*.mo'), glob(dirname(__DIR__, 4) . '/tests/php/data/pomo/*.mo'),
    glob(dirname(__DIR__, 4) . '/tests/php/data/l10n/*.mo'));
sort($files);
$out = array();
foreach ($files as $file) {
    $name = basename(dirname($file)) . '/' . basename($file);
    $mo = new SpeedySpec_Recording_MO();
    $result = $mo->import_from_file($file);
    // What export_to_file_handle() packs.
    $entries = array_filter($mo->entries, array($mo, 'is_entry_good_for_export'));
    ksort($entries);
    $pack = array();
    foreach ($entries as $e) {
        $pack[] = array(bin2hex($mo->export_original($e)), bin2hex($mo->export_translations($e)));
    }
    $case = array('result' => $result, 'events' => speedyspec_small($mo->events), 'pack' => speedyspec_small($pack),
        'headers' => bin2hex($mo->export_headers()), 'export' => speedyspec_bytes($mo->export()));
    $file_object = WP_Translation_File::create($file);
    if ($file_object) {
        $case['file_headers'] = speedyspec_hex_pairs($file_object->headers());
        $case['file_entries'] = speedyspec_small(speedyspec_hex_pairs($file_object->entries()));
        $case['file_error'] = $file_object->error();
        foreach (array('mo', 'php') as $type) {
            $t = WP_Translation_File::transform($file, $type);
            $case["transform_$type"] = speedyspec_bytes($t);
        }
    }
    $out[$name] = $case;
}
$values = array('', 'plain', "it's", 'back\\slash', "nul\0byte", "\0", "\xff\xfe", "\"double\"", "multi\nline", "\\'");
$keys = array('0' => 'zero', '01' => 'leading', '-1' => 'negative', '1.5' => 'float', 'k' => 'string', 7 => 'int');
$file = (new ReflectionClass('WP_Translation_File_PHP'))->newInstanceWithoutConstructor();
$ref = new ReflectionClass($file);
$method = $ref->getMethod('var_export');
$var = array();
foreach ($values as $v) {
    $var[] = array(bin2hex($v), bin2hex($method->invoke($file, $v)));
}
$var[] = array('keys', bin2hex($method->invoke($file, $keys)));
$var[] = array('list', bin2hex($method->invoke($file, array('a', "b\0c"))));
file_put_contents(__DIR__ . '/mo-recorded.json', json_encode(array('files' => $out, 'var_export' => $var), JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n");
echo count($out), " files\n";
