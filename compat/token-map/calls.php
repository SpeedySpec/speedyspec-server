<?php
/**
 * The calls record.php and the port's test both make, in order. Each result is serialize()d, so types count too;
 * a thrown error is recorded as its class and message, and warnings raised during a call are recorded with it.
 */

function speedyspec_token_map_try(callable $f) {
    $warnings = array();
    set_error_handler(function ($errno, $errstr) use (&$warnings) { $warnings[] = $errstr; return true; });
    try {
        $result = $f();
    } catch (Throwable $e) {
        $result = 'threw ' . get_class($e) . ': ' . $e->getMessage();
    } finally {
        restore_error_handler();
    }
    return base64_encode(serialize(array($result, $warnings)));
}

/** `read_token()` with its out-parameter, as one value. */
function speedyspec_token_map_read($map, string $text, int $offset, string $case = 'case-sensitive'): array {
    $length = 'unset';
    $found = $map->read_token($text, $offset, $length, $case);
    return array($found, $length);
}

function speedyspec_token_map_calls(string $entities_file): array {
    global $html5_named_character_references;
    $map = $html5_named_character_references;
    $out = array();
    $r = function ($name, callable $f) use (&$out) {
        $out[$name] = speedyspec_token_map_try($f);
    };
    $entities = array();
    foreach (json_decode(file_get_contents($entities_file), true) as $name => $value) {
        $entities[substr($name, 1)] = $value['characters'];
    }
    // Every name, by its first byte: followed by nothing, by 'x' and by ';', case-sensitively and not.
    $by_first = array();
    foreach (array_keys($entities) as $name) {
        $by_first[$name[0]][] = $name;
    }
    foreach ($by_first as $first => $names) {
        $r("read:$first", function () use ($map, $names) {
            $res = array();
            foreach ($names as $name) {
                $res[] = array(
                    speedyspec_token_map_read($map, $name, 0),
                    speedyspec_token_map_read($map, $name . 'x', 0),
                    speedyspec_token_map_read($map, "&$name;", 1),
                    speedyspec_token_map_read($map, strtoupper($name), 0, 'ascii-case-insensitive'),
                    speedyspec_token_map_read($map, strtolower($name) . 'x', 0, 'ascii-case-insensitive'),
                );
            }
            return $res;
        });
        $r("contains:$first", function () use ($map, $names) {
            $res = array();
            foreach ($names as $name) {
                $res[] = array($map->contains($name), $map->contains(str_replace('&', '&amp;', $name)),
                    $map->contains(strtoupper($name)), $map->contains(strtoupper($name), 'ascii-case-insensitive'),
                    $map->contains($name . "\0"), $map->contains(substr($name, 0, -1)));
            }
            return $res;
        });
    }
    $texts = array('', '&', 'a', 'l', 'lt', 'Lt', 'LT', "l\0t", "\0lt", 'amp', 'ampx', 'AMP;', 'notin;', 'notit;', 'not', "ab\0", 'gt;x', 'xx');
    foreach ($texts as $i => $text) {
        $r("short:$i", function () use ($map, $text) {
            $res = array();
            for ($offset = 0; $offset <= strlen($text); $offset++) {
                $res[] = array(speedyspec_token_map_read($map, $text, $offset), speedyspec_token_map_read($map, $text, $offset, 'ascii-case-insensitive'),
                    speedyspec_token_map_read($map, $text, $offset, 'other'));
            }
            return $res;
        });
    }
    $animals = array('cat' => '🐈', 'dog' => '🐶', 'fish' => '🐟', 'mammoth' => '🦣', 'seal' => '🦭');
    $mixed = array('a' => '1', 'ab' => '2', 'abc' => '3', 'abcd' => '4', 'B' => '5', 'bc' => '6', 'x' => "\0", 'xy' => 'q"\\', 'Zz' => '7');
    foreach (array('animals' => $animals, 'mixed' => $mixed) as $set => $words) {
        foreach (array(1, 2, 3) as $key_length) {
            $r("map:$set:$key_length", function () use ($words, $key_length) {
                $m = WP_Token_Map::from_array($words, $key_length);
                $reads = array();
                foreach (array('cat', 'catx', 'CAT', 'abcde', 'ABC', 'b', 'bcd', 'BC', 'xyz', 'XY', 'zz', 'Zz', 'mammoths', 'q', '') as $text) {
                    $reads[] = array(speedyspec_token_map_read($m, $text, 0), speedyspec_token_map_read($m, $text, 0, 'ascii-case-insensitive'),
                        $m->contains($text), $m->contains($text, 'ascii-case-insensitive'));
                }
                return array($m->to_array(), $m->precomputed_php_source_table(), $reads);
            });
        }
    }
    $r('map:too-long', function () {
        $GLOBALS['speedyspec_wrong'] = array();
        $a = WP_Token_Map::from_array(array(str_repeat('a', 256) => 'x'));
        $b = WP_Token_Map::from_array(array('a' => str_repeat('x', 256)));
        $c = WP_Token_Map::from_array(array(str_repeat('a', 255) => str_repeat('x', 255)));
        return array($a, $b, $c instanceof WP_Token_Map, $GLOBALS['speedyspec_wrong']);
    });
    $r('map:precomputed', function () use ($map) {
        $GLOBALS['speedyspec_wrong'] = array();
        $missing = WP_Token_Map::from_precomputed_table(array('storage_version' => '6.6.0-trunk'));
        $version = WP_Token_Map::from_precomputed_table(array('storage_version' => '1', 'key_length' => 2, 'groups' => '', 'large_words' => array(), 'small_words' => '', 'small_mappings' => array()));
        return array($missing, $version, $GLOBALS['speedyspec_wrong'], md5(serialize($map->to_array())), count($map->to_array()));
    });
    $r('map:html5-source', function () use ($map) {
        return md5($map->precomputed_php_source_table());
    });
    return $out;
}
