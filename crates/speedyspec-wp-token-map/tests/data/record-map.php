<?php
/**
 * Records WordPress's WP_Token_Map into map-recorded.json, for the Rust port's tests: maps built by from_array() at
 * several key lengths, then read_token() at every offset of a set of texts (both case modes, and an unknown mode
 * string), contains(), and to_array(). Byte strings are hex; warnings WordPress raises are not recorded.
 *
 * Usage: .php-build/buildroot/bin/php record-map.php <wordpress-root>
 */

[$_, $root] = $argv + [null, null];
function __($t) { return $t; }
function _doing_it_wrong() {}
require rtrim($root, '/') . '/wp-includes/class-wp-token-map.php';
error_reporting(E_ALL & ~E_WARNING & ~E_DEPRECATED);

$entities = array();
foreach (json_decode(file_get_contents(dirname(__DIR__, 4) . '/tests/php/data/html5-entities/entities.json'), true) as $name => $v) {
    $entities[substr($name, 1)] = $v['characters'];
}
$sets = array(
    'animals' => array('cat' => '🐈', 'dog' => '🐶', 'fish' => '🐟', 'mammoth' => '🦣', 'seal' => '🦭'),
    'mixed' => array('a' => '1', 'ab' => '2', 'abc' => '3', 'abcd' => '4', 'B' => '5', 'bc' => '6', 'x' => "\0", 'xy' => 'q"\\',
        'Zz' => '7', 'zZq' => '8', '12' => 'twelve', '9' => 'nine', '012' => 'oh-twelve', '1x' => 'one-x', '' => 'empty'),
);
$texts = array('', 'a', 'ab', 'abc', 'abcde', 'ABC', 'AbC', 'b', 'B', 'bc', 'BCD', 'x', 'xy', 'XYZ', "x\0y", "\0x", 'zz', 'ZZQ', 'zzq',
    'cat', 'CATS', 'mammoths', 'seal', '12', '123', '9', '012', '1xx', 'amp;', 'AMP;', 'notin;', 'not', 'lt', 'LT', 'Lt', 'ampx', 'q');
$out = array();
$build = array();
foreach ($sets as $set => $words) {
    foreach (array(1, 2, 3) as $kl) {
        $build["$set:$kl"] = array($words, $kl);
    }
}
$build['html5:2'] = array($entities, 2);
foreach ($build as $name => list($words, $kl)) {
    $map = WP_Token_Map::from_array($words, $kl);
    $case = array('words' => array(), 'key_length' => $kl, 'reads' => array(), 'contains' => array(), 'to_array' => array());
    foreach ($words as $w => $m) {
        $case['words'][] = array(bin2hex((string) $w), bin2hex($m));
    }
    $list = 'html5:2' === $name ? array_merge($texts, array_map(function ($w) { return $w . 'x'; }, array_keys($entities))) : $texts;
    foreach ($list as $text) {
        $text = (string) $text;
        $offsets = 'html5:2' === $name && strlen($text) > 6 ? array(0) : range(0, strlen($text));
        foreach ($offsets as $offset) {
            $modes = 'html5:2' === $name && !in_array($text, $texts, true) ? array('case-sensitive') : array('case-sensitive', 'ascii-case-insensitive', 'other');
            foreach ($modes as $mode) {
                $len = null;
                $found = $map->read_token($text, $offset, $len, $mode);
                $case['reads'][] = array(bin2hex($text), $offset, $mode, null === $found ? null : bin2hex($found), $len);
            }
        }
        foreach ('html5:2' === $name && !in_array($text, $texts, true) ? array('case-sensitive') : array('case-sensitive', 'ascii-case-insensitive') as $mode) {
            $case['contains'][] = array(bin2hex($text), $mode, $map->contains($text, $mode));
        }
    }
    foreach ($map->to_array() as $k => $v) {
        $case['to_array'][] = array(bin2hex((string) $k), bin2hex($v));
    }
    $out[$name] = $case;
}
file_put_contents(__DIR__ . '/map-recorded.json', json_encode($out, JSON_UNESCAPED_SLASHES) . "\n");
echo count($out), " maps\n";
