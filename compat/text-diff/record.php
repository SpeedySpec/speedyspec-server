<?php
/**
 * Records WordPress's text diff behaviour into fixtures the Rust port is tested against.
 *
 * Usage: php record.php <wordpress-root> <out-dir>
 *
 * Run it with the static PHP the port embeds (`.php-build/buildroot/bin/php`), so PCRE and the string functions are
 * the same build. Every input is generated here from a fixed seed and from the release's own files, so a re-record
 * only changes when WordPress or PHP changes.
 */

declare(strict_types=1);

[$_, $root, $out] = $argv + [null, null, null];
if (!$root || !$out) {
    fwrite(STDERR, "usage: php record.php <wordpress-root> <out-dir>\n");
    exit(2);
}
$root = rtrim($root, '/') . '/';
define('ABSPATH', $root);
define('WPINC', 'wp-includes');
error_reporting(E_ALL & ~E_DEPRECATED);

require ABSPATH . WPINC . '/plugin.php';
require ABSPATH . WPINC . '/load.php';
require ABSPATH . WPINC . '/functions.php';
require ABSPATH . WPINC . '/formatting.php';
require ABSPATH . WPINC . '/pomo/translations.php';
require ABSPATH . WPINC . '/l10n.php';
require ABSPATH . WPINC . '/pluggable.php';
require ABSPATH . WPINC . '/wp-diff.php';

/** JSON-safe string: as is when valid UTF-8, `{"b64": ...}` otherwise. */
function enc($s)
{
    if (!is_string($s)) {
        return $s;
    }
    return mb_check_encoding($s, 'UTF-8') ? $s : array('b64' => base64_encode($s));
}

function ops($a, $b)
{
    $d = new Text_Diff('native', array($a, $b));
    $o = array();
    foreach ($d->getDiff() as $e) {
        $o[] = array(substr(get_class($e), 13), $e->orig ? count($e->orig) : 0, $e->final ? count($e->final) : 0);
    }
    return $o;
}

function words($s)
{
    return preg_split('/([^\w])/u', str_replace("\0", '', $s), -1, PREG_SPLIT_DELIM_CAPTURE);
}

function write_json($path, $value, $pretty = true)
{
    $json = json_encode($value, ($pretty ? JSON_PRETTY_PRINT : 0) | JSON_UNESCAPED_SLASHES | JSON_UNESCAPED_UNICODE | JSON_THROW_ON_ERROR);
    file_put_contents($path, $json . "\n");
}

mt_srand(42);
$vocab = explode(' ', 'the a of to and in is it you that he was for on are with as I his they be at one have this from or had by hot word but what some we can out other were all there when up use your how said an each she which do their time if will way about many then them write would like so these her long make thing see him two has look more day could go come did number sound no most people my over know water than call first who may down side been now find café naïve über Ελλάδα 日本語 x́y 1000 1e3 1.0 0x1A <b> & "q" \'s\' &amp;');
$sources = array();
foreach (array('wp-includes/formatting.php', 'wp-includes/post.php', 'wp-includes/class-wp-query.php', 'readme.html', 'license.txt') as $f) {
    $sources[] = file(ABSPATH . $f, FILE_IGNORE_NEW_LINES);
}

function sentence($vocab, $max)
{
    $w = array();
    for ($k = 0, $m = mt_rand(0, $max); $k < $m; $k++) {
        $w[] = $vocab[mt_rand(0, count($vocab) - 1)];
    }
    return implode(' ', $w);
}

/** Old and new texts: a slice of a release file or generated prose, with a few random edits. */
function generated_pairs($n, $sources, $vocab)
{
    $pairs = array();
    for ($i = 0; $i < $n; $i++) {
        if ($i % 3 === 0) {
            $a = array();
            for ($j = 0, $len = mt_rand(1, 30); $j < $len; $j++) {
                $a[] = sentence($vocab, 14);
            }
        } else {
            $f = $sources[$i % count($sources)];
            $len = min(count($f), mt_rand(2, $i % 10 === 1 ? 150 : 30));
            $a = array_slice($f, mt_rand(0, count($f) - $len), $len);
        }
        $b = $a;
        for ($e = 0, $ne = mt_rand(1, 6); $e < $ne; $e++) {
            $p = mt_rand(0, max(0, count($b) - 1));
            switch (mt_rand(0, 6)) {
                case 0: array_splice($b, $p, 1); break;
                case 1: array_splice($b, $p, 0, array(sentence($vocab, 6))); break;
                case 2:
                    if (isset($b[$p])) {
                        $ws = explode(' ', $b[$p]);
                        $ws[mt_rand(0, count($ws) - 1)] = $vocab[mt_rand(0, count($vocab) - 1)];
                        $b[$p] = implode(' ', $ws);
                    }
                    break;
                case 3: if (isset($b[$p])) { $b[$p] = ''; } break;
                case 4:
                    if (isset($b[$p])) {
                        $q = mt_rand(0, count($b) - 1);
                        $t = $b[$p]; $b[$p] = $b[$q]; $b[$q] = $t;
                    }
                    break;
                case 5: if (isset($b[$p])) { $b[$p] = $b[$p] . ' ' . sentence($vocab, 3); } break;
                default: array_splice($b, $p, 0, array($b[$p] ?? 'x')); break;
            }
        }
        $pairs[] = array(implode("\n", $a), implode("\n", $b));
    }
    return $pairs;
}

$generated = generated_pairs(400, $sources, $vocab);

// Quirk cases for the engine: numeric-looking tokens, repeats, a match at index 0, CR/LF inside tokens.
$quirks = array(
    array(array('1e3', 'a', 'b'), array('1000', 'a', 'c')),
    array(array('x', '1.0', 'y', 'z'), array('w', '1', 'y', 'q')),
    array(array(' 1', 'a', 'b', 'c'), array('1', 'b', 'a', 'c')),
    array(array('a', 'b', 'a', 'b', 'a'), array('b', 'a', 'b', 'a', 'b')),
    array(array('a', 'x', 'a', 'x', 'a', 'x'), array('x', 'a', 'x', 'a')),
    array(array('q', 'r', 's', 't'), array('t', 'q', 'r')),
    array(array("a\r", 'b', "c\n"), array('a', 'b', 'c')),
    array(array('0', '00', '0.0', '-0'), array('00', '0', '-0', '0.0')),
    array(array('', '', 'a', ''), array('a', '', '', '')),
    array(array('INF', 'NAN', '1e500'), array('1e400', 'NAN', 'INF')),
    array(array('9223372036854775807', '9223372036854775808'), array('9223372036854775808', '9.2233720368547758E+18')),
    array(array('a', 'b', 'c', 'd', 'e', 'f', 'g', 'h', 'i', 'j'), array('j', 'i', 'h', 'g', 'f', 'e', 'd', 'c', 'b', 'a')),
);
for ($i = 0; $i < 200; $i++) {
    $alphabet = array('a', 'b', 'c', '1', '1.0', '01', '1e0', ' 1', '');
    $x = array(); $y = array();
    for ($k = 0, $m = mt_rand(0, 12); $k < $m; $k++) { $x[] = $alphabet[mt_rand(0, count($alphabet) - 1)]; }
    for ($k = 0, $m = mt_rand(0, 12); $k < $m; $k++) { $y[] = $alphabet[mt_rand(0, count($alphabet) - 1)]; }
    $quirks[] = array($x, $y);
}

// --- engine.json: op scripts of Text_Diff_Engine_native.
$engine = array();
foreach ($quirks as $q) {
    $engine[] = array('orig' => array_map('enc', $q[0]), 'final' => array_map('enc', $q[1]), 'ops' => ops($q[0], $q[1]));
}
foreach ($generated as $g) {
    $a = explode("\n", $g[0]);
    $b = explode("\n", $g[1]);
    $engine[] = array('orig' => array_map('enc', $a), 'final' => array_map('enc', $b), 'ops' => ops($a, $b));
    for ($i = 0; $i < min(count($a), count($b)); $i++) {
        if ($a[$i] !== $b[$i] && $a[$i] !== '' && $b[$i] !== '') {
            $wa = words($a[$i]);
            $wb = words($b[$i]);
            $engine[] = array('orig' => array_map('enc', $wa), 'final' => array_map('enc', $wb), 'ops' => ops($wa, $wb));
        }
    }
}
write_json("$out/engine.json", $engine, false);

// --- units.json: the leaf functions.
$samples = array('', 'plain', '<b>"x" & \'y\'</b>', '&amp; &#039;', "a\xffb", "\xc3", "\xe2\x82", "ok\xed\xa0\x80", 'café naïve', "x\u{0301}y", "tab\there", "nul\0byte", 'a_b-c.d', '日本語のテキスト', 'Ελλάδα!', "\u{2028}line", '٣٤ digits', 'emoji 😀 here', "mixed\xc0\xafbad", str_repeat('<&>"\'', 20), str_repeat('abcdefghij', 10) . '<' . str_repeat('k', 33) . '&');
for ($i = 0; $i < 300; $i++) {
    $s = '';
    for ($k = 0, $m = mt_rand(0, 70); $k < $m; $k++) {
        $r = mt_rand(0, 9);
        $s .= $r < 5 ? chr(mt_rand(32, 126)) : ($r < 7 ? chr(mt_rand(0, 255)) : $vocab[mt_rand(0, count($vocab) - 1)]);
    }
    $samples[] = $s;
}
$units = array('escape' => array(), 'words' => array(), 'loose' => array(), 'distance' => array(), 'normalize' => array());
foreach ($samples as $s) {
    $units['escape'][] = array(enc($s), enc(htmlspecialchars($s)));
    $w = words($s);
    $units['words'][] = array(enc($s), false === $w ? false : array_map('enc', $w));
    $units['normalize'][] = array(enc($s), enc(normalize_whitespace($s)));
}
$numeric = array('1', '1.0', '01', '1e0', ' 1', '1 ', ' 1 ', "\t1\n", '+1', '-1', '.5', '0.5', '5.', '1e3', '1000', '1E3', '0x1A', '26', '1_000', '', ' ', '0', '-0', '0.0', '00', 'INF', 'NAN', '1e500', '1e400', '-1e500', '9223372036854775807', '9223372036854775808', '9.2233720368547758E+18', '1.5e3', '1500', 'abc', '1a', 'a1', "1\0", "\v1", "\f1", '١');
foreach ($numeric as $a) {
    foreach ($numeric as $b) {
        $units['loose'][] = array($a, $b, $a == $b);
    }
}
$renderer = new WP_Text_Diff_Renderer_Table();
foreach (array_slice($samples, 0, 120) as $i => $s) {
    $t = $samples[($i * 7 + 3) % count($samples)];
    $units['distance'][] = array(enc($s), enc($t), $renderer->compute_string_distance($s, $t));
}
write_json("$out/units.json", $units);

// --- inline.json: WordPress's inline word diff of one line against another.
$inline = array();
foreach ($generated as $g) {
    $a = explode("\n", $g[0]);
    $b = explode("\n", $g[1]);
    for ($i = 0; $i < min(count($a), count($b)); $i++) {
        if ($a[$i] !== $b[$i] && mb_check_encoding($a[$i] . $b[$i], 'UTF-8')) {
            $d = new Text_Diff('auto', array(array($a[$i]), array($b[$i])));
            $r = new WP_Text_Diff_Renderer_inline();
            $inline[] = array(enc($a[$i]), enc($b[$i]), enc($r->render($d)));
        }
    }
}
write_json("$out/inline.json", $inline, false);

// --- cases.json: wp_text_diff() itself.
$hand = array(
    array('identical', 'same', 'same', null),
    array('both_empty', '', '', null),
    array('left_empty', '', "a\nb", null),
    array('right_empty', "a\nb", '', null),
    array('one_word', 'Hello world', 'Hello there world', null),
    array('title', "a\nb", "a\nc", array('title' => 'Title <em>x</em>')),
    array('title_zero', "a\nb", "a\nc", array('title' => '0', 'title_left' => '0', 'title_right' => 'R')),
    array('titles_both', "a\nb", "a\nc", array('title_left' => 'Old', 'title_right' => 'New')),
    array('title_left_only', "a\nb", "a\nc", array('title_left' => 'Old')),
    array('title_right_only', "a\nb", "a\nc", array('title_right' => 'New')),
    array('unified', "a\nb\nc", "a\nB\nc\nd", array('show_split_view' => false)),
    array('unified_titles', "a\nb", "a\nc", array('show_split_view' => 0, 'title_left' => 'Old', 'title_right' => 'New')),
    array('split_string_zero', "a\nb", "a\nc", array('show_split_view' => '0')),
    array('query_string_args', "a\nb", "a\nc", 'title=Query&show_split_view=0'),
    array('object_args', "a\nb", "a\nc", (object) array('title' => 'Obj')),
    array('special_chars', "<p class=\"x\">Tom & 'Jerry'</p>", "<p class=\"y\">Tom &amp; 'Jerry'</p>", null),
    array('whitespace_normalized', "a  b\r\n\r\n\tc  ", "a b\nc", null),
    array('whitespace_change', "a\n\nb", "a\nb\n\n\nc", null),
    array('unicode', "café\nnaïve über", "cafe\nnaive über", null),
    array('combining', "x\u{0301}y z", "x\u{0301}y w", null),
    array('too_different', 'abcdefghijklmnop', 'qrstuvwxyz0123456789', null),
    array('threshold_edge', 'aaaa bbbb cccc', 'aaaa dddd eeee', null),
    array('many_to_one', "alpha beta\ngamma delta\nepsilon", "alpha beta gamma", null),
    array('one_to_many', 'alpha beta gamma', "alpha beta\ngamma delta\nepsilon zeta", null),
    array('reorder', "one\ntwo\nthree\nfour", "four\nthree\ntwo\none", null),
    array('numeric_lines', "1000\n2\n3", "1e3\n2\n4", null),
    array('nul_bytes', "a\0b c", "a\0b d", null),
    array('leading_spaces', ' x y', ' x z', null),
    array('long_line', str_repeat('word ', 300) . 'end', str_repeat('word ', 150) . 'middle ' . str_repeat('word ', 150) . 'end', null),
);
$cases = array();
foreach ($hand as $h) {
    $cases[] = array('name' => $h[0], 'left' => enc($h[1]), 'right' => enc($h[2]), 'args' => $h[3], 'output' => enc(wp_text_diff($h[1], $h[2], $h[3])));
}
foreach ($generated as $i => $g) {
    $args = $i % 4 === 1 ? array('show_split_view' => false) : null;
    $cases[] = array('name' => "generated_$i", 'left' => enc($g[0]), 'right' => enc($g[1]), 'args' => $args, 'output' => enc(wp_text_diff($g[0], $g[1], $args)));
}
// Filter cases: the callback fixtures are named; the replayer installs the same callback.
$filters = array(
    'wrap' => function ($processed, $line, $kind) { return "[$kind]$processed"; },
    'raw' => function ($processed, $line, $kind) { return $line; },
    'int' => function ($processed, $line, $kind) { return 7; },
);
foreach ($filters as $name => $callback) {
    add_filter('process_text_diff_html', $callback, 10, 3);
    $l = "keep\n<old>\nsame & co";
    $r = "keep\n<new>\nsame & co\nadded";
    $cases[] = array('name' => "filter_$name", 'filter' => $name, 'left' => $l, 'right' => $r, 'args' => null, 'output' => wp_text_diff($l, $r), 'did_filter' => did_filter('process_text_diff_html'));
    remove_all_filters('process_text_diff_html');
}
// Invalid UTF-8 in a changed pair: WordPress throws.
foreach (array(array("a\xffb c", 'a b d'), array('x', "x\xff")) as $i => $p) {
    try {
        $o = wp_text_diff($p[0], $p[1]);
        $cases[] = array('name' => "invalid_utf8_$i", 'left' => enc($p[0]), 'right' => enc($p[1]), 'args' => null, 'output' => enc($o));
    } catch (Throwable $t) {
        $cases[] = array('name' => "invalid_utf8_$i", 'left' => enc($p[0]), 'right' => enc($p[1]), 'args' => null, 'throws' => get_class($t));
    }
}
write_json("$out/cases.json", $cases);

fwrite(STDERR, sprintf("engine %d, inline %d, cases %d\n", count($engine), count($inline), count($cases)));
