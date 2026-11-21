<?php
/**
 * Writes the damaged and unusual MO fixtures in mo/ (committed with this script): files WordPress's MO reader and
 * WP_Translation_File_MO must answer as WordPress does.
 *
 * Usage: php make-mo-fixtures.php
 */

/**
 * An MO file's bytes: the header, the two offset tables, then the strings.
 *
 * @param array<int, array{string, string}> $pairs
 *     Originals and translations, in table order.
 * @param string $endian
 *     'V' (little) or 'N' (big).
 * @param int $revision
 *     The revision field.
 * @return string
 *     The file.
 */
function speedyspec_mo(array $pairs, string $endian = 'V', int $revision = 0): string {
    $n = count($pairs);
    $o_table = 28;
    $t_table = $o_table + 8 * $n;
    $addr = $t_table + 8 * $n;
    $head = pack($endian . '7', 0x950412de, $revision, $n, $o_table, $t_table, 0, $addr);
    $o = $t = $strings = '';
    foreach ($pairs as [$orig]) {
        $o .= pack($endian . '2', strlen($orig), $addr);
        $strings .= $orig . "\0";
        $addr += strlen($orig) + 1;
    }
    foreach ($pairs as [, $tr]) {
        $t .= pack($endian . '2', strlen($tr), $addr);
        $strings .= $tr . "\0";
        $addr += strlen($tr) + 1;
    }
    return $head . $o . $t . $strings;
}

$header = array('', "Content-Type: text/plain; charset=UTF-8\nPlural-Forms: nplurals=2; plural=n != 1;\n");
$dir = __DIR__ . '/mo';
file_put_contents("$dir/revision1.mo", speedyspec_mo(array($header, array('a', 'b')), 'V', 1));
file_put_contents("$dir/big-endian-plural.mo", speedyspec_mo(array($header, array("cat\0cats", "Katze\0Katzen"), array("ctx\x04door", 'Tür')), 'N'));
file_put_contents("$dir/no-header.mo", speedyspec_mo(array(array('a', 'b'), array('c', 'd'))));
$full = speedyspec_mo(array($header, array('a', 'b'), array('c', 'd')));
file_put_contents("$dir/truncated.mo", substr($full, 0, 40));
$past = speedyspec_mo(array($header, array('a', 'b')));
// The second original's offset points past the end of the file.
$past = substr_replace($past, pack('V', 9999), 28 + 8 + 4, 4);
file_put_contents("$dir/offsets-past-end.mo", $past);
echo "written\n";
