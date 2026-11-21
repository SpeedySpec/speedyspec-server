<?php
/**
 * Records WordPress's PO reader and helpers into po-recorded.json, for the Rust PO port's tests: every
 * PO::read_entry() result for each fixture (as WordPress's import_from_file() loop reads them), and the static
 * helpers over edge strings. Byte strings are hex.
 *
 * Usage: .php-build/buildroot/bin/php record-po.php <wordpress-root>
 */

[$_, $root] = $argv + [null, null];
require rtrim($root, '/') . '/wp-includes/pomo/po.php';

function speedyspec_hex($v) {
    if (is_string($v)) {
        return bin2hex($v);
    }
    if (is_array($v)) {
        $out = array();
        foreach ($v as $k => $x) {
            $out[] = array($k, speedyspec_hex($x));
        }
        return $out;
    }
    return $v;
}

function speedyspec_entry(Translation_Entry $e): array {
    return array(
        'context' => speedyspec_hex($e->context),
        'singular' => speedyspec_hex($e->singular),
        'plural' => speedyspec_hex($e->plural),
        'translations' => speedyspec_hex($e->translations),
        'is_plural' => $e->is_plural,
        'translator_comments' => speedyspec_hex($e->translator_comments),
        'extracted_comments' => speedyspec_hex($e->extracted_comments),
        'references' => speedyspec_hex($e->references),
        'flags' => speedyspec_hex($e->flags),
    );
}

$files = array_merge(glob(__DIR__ . '/po/*.po'), glob(dirname(__DIR__, 4) . '/tests/php/data/pomo/*.po*'));
sort($files);
$out = array('files' => array(), 'static' => array());
foreach ($files as $file) {
    $po = new PO();
    $f = fopen($file, 'r');
    $entries = array();
    while (true) {
        $res = $po->read_entry($f);
        if (!$res) {
            break;
        }
        $entries[] = speedyspec_entry($res['entry']);
    }
    $po->read_line($f, 'clear');
    $out['files'][basename(dirname($file)) . '/' . basename($file)] = array('entries' => $entries, 'ended' => var_export($res, true));
}

$strings = array('', 'plain', "a\nb", "a\nb\n", "\n", "\n\n", "tab\there", "back\\slash", 'quote"d', "cr\rhere",
    "trailing space \n", "\xff\xfe bytes", "multi\nline\nwith \"quotes\" and \\", "ends\\", 'é\\é', '"quoted"',
    str_repeat('word ', 40), str_repeat('x', 100) . ' tail', "short\n" . str_repeat('long ', 30));
foreach ($strings as $s) {
    $out['static'][] = array('poify', array(bin2hex($s)), bin2hex(PO::poify($s)));
    $out['static'][] = array('unpoify', array(bin2hex('"' . $s . '"')), bin2hex(PO::unpoify('"' . $s . '"')));
    $out['static'][] = array('unpoify', array(bin2hex(PO::poify($s))), bin2hex(PO::unpoify(PO::poify($s))));
    $out['static'][] = array('prepend_each_line', array(bin2hex($s), bin2hex('#. ')), bin2hex(PO::prepend_each_line($s, '#. ')));
    $out['static'][] = array('comment_block', array(bin2hex($s), bin2hex(' ')), bin2hex(PO::comment_block($s)));
    $out['static'][] = array('comment_block', array(bin2hex($s), bin2hex(':')), bin2hex(PO::comment_block($s, ':')));
    $out['static'][] = array('trim_quotes', array(bin2hex('"' . $s . '"')), bin2hex(PO::trim_quotes('"' . $s . '"')));
    $out['static'][] = array('match_begin_and_end_newlines', array(bin2hex($s), bin2hex("\n" . $s)), bin2hex(PO::match_begin_and_end_newlines($s, "\n" . $s)));
    $out['static'][] = array('match_begin_and_end_newlines', array(bin2hex("\n$s\n"), bin2hex('x')), bin2hex(PO::match_begin_and_end_newlines("\n$s\n", 'x')));
}
$entries = array(
    array('singular' => 'a', 'translations' => array('b')),
    array('singular' => "multi\nline", 'translations' => array("viele\nZeilen"), 'context' => 'ctx'),
    array('singular' => 'cat', 'plural' => 'cats', 'translations' => array('Katze', 'Katzen')),
    array('singular' => 'cat', 'plural' => 'cats'),
    array('singular' => 'r', 'references' => array('a.php:1', 'b.php:2'), 'flags' => array('fuzzy', 'php-format'),
        'translator_comments' => "one\ntwo", 'extracted_comments' => 'extracted'),
    array('singular' => "\nlead", 'translations' => array('no lead')),
    array('singular' => 'x', 'context' => '0', 'translator_comments' => '0', 'translations' => array('0')),
);
foreach ($entries as $args) {
    $e = new Translation_Entry($args);
    $r = PO::export_entry($e);
    $out['export_entry'][] = array(speedyspec_entry($e), false === $r ? false : bin2hex($r));
}
file_put_contents(__DIR__ . '/po-recorded.json', json_encode($out, JSON_PRETTY_PRINT | JSON_UNESCAPED_SLASHES) . "\n");
echo count($out['files']), " files, ", count($out['static']), " helper calls\n";
