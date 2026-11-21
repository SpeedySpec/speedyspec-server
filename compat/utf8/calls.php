<?php
/**
 * The calls record.php and replay.php both make for one input, keyed by name; each result is serialize()d so types
 * (an int 0 against false, say) must match too.
 *
 * Expects the UTF-8 functions to be defined, and _deprecated_function() to record into $GLOBALS['speedyspec_deprecated'].
 */

function speedyspec_utf8_calls(string $s): array
{
    $out = array();
    $out['wp_is_valid_utf8'] = wp_is_valid_utf8($s);
    $out['wp_scrub_utf8'] = wp_scrub_utf8($s);
    $out['wp_has_noncharacters'] = wp_has_noncharacters($s);
    foreach (array(array(0, null, null), array(1, 3, null), array(0, null, 2), array(2, 1, 1)) as $k => list($at, $max_bytes, $max_cp)) {
        $invalid = -1;
        $nc = null;
        $n = _wp_scan_utf8($s, $at, $invalid, $max_bytes, $max_cp, $nc);
        $out["_wp_scan_utf8#$k"] = array($n, $at, $invalid, $nc);
    }
    $out['_wp_is_valid_utf8_fallback'] = _wp_is_valid_utf8_fallback($s);
    $out['_wp_scrub_utf8_fallback'] = _wp_scrub_utf8_fallback($s);
    $GLOBALS['speedyspec_deprecated'] = array();
    $out['_wp_has_noncharacters_fallback'] = array(_wp_has_noncharacters_fallback($s), $GLOBALS['speedyspec_deprecated']);
    $out['_wp_utf8_codepoint_count#0'] = _wp_utf8_codepoint_count($s);
    $out['_wp_utf8_codepoint_count#1'] = _wp_utf8_codepoint_count($s, 1, 5);
    $out['_wp_utf8_codepoint_count#2'] = _wp_utf8_codepoint_count($s, null, null);
    $out['_wp_utf8_codepoint_count#3'] = _wp_utf8_codepoint_count($s, -1);
    $found = -1;
    $out['_wp_utf8_codepoint_span#0'] = array(_wp_utf8_codepoint_span($s, 0, 3, $found), $found);
    $out['_wp_utf8_codepoint_span#1'] = array(_wp_utf8_codepoint_span($s, 1, 2, $found), $found);
    $out['_wp_utf8_encode_fallback'] = _wp_utf8_encode_fallback($s);
    $out['_wp_utf8_decode_fallback'] = _wp_utf8_decode_fallback($s);
    $out['_mb_substr#0'] = _mb_substr($s, 1, 3, 'UTF-8');
    $out['_mb_substr#1'] = _mb_substr($s, -3, null, 'utf8');
    $out['_mb_substr#2'] = _mb_substr($s, 2, -1, '8bit');
    $out['_mb_substr#3'] = _mb_substr($s, -2, -1, 'UTF-8');
    $out['_mb_strlen#0'] = _mb_strlen($s, 'UTF-8');
    $out['_mb_strlen#1'] = _mb_strlen($s, 'latin1');
    $out['_is_utf8_charset'] = array(_is_utf8_charset($s), _is_utf8_charset('Utf-8'), _is_utf8_charset(8));
    return array_map(function ($v) {
        return base64_encode(serialize($v));
    }, $out);
}

/** The inputs: the decoder stress cases, then 600 seeded random byte strings. */
function speedyspec_utf8_inputs(string $cases_file): array
{
    $inputs = array();
    foreach (file($cases_file, FILE_IGNORE_NEW_LINES) as $line) {
        $line = trim($line);
        if ('' !== $line && '#' !== $line[0]) {
            $inputs[] = hex2bin($line);
        }
    }
    $inputs[] = '';
    $inputs[] = "a\0b";
    $inputs[] = 'баба I am your баба 日本語テキストです';
    mt_srand(7);
    $alphabet = array_merge(range(0x00, 0x7F, 9), range(0x80, 0xBF), range(0xC0, 0xFF), array(0x20, 0x61, 0xEF, 0xB7, 0xBF, 0xF0, 0xF4, 0x8F));
    for ($i = 0; $i < 600; $i++) {
        $s = '';
        $len = mt_rand(0, 12);
        for ($j = 0; $j < $len; $j++) {
            $s .= chr($alphabet[mt_rand(0, count($alphabet) - 1)]);
        }
        $inputs[] = $s;
    }
    return $inputs;
}
