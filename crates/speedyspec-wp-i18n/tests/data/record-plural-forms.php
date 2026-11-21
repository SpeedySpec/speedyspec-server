<?php
/**
 * Records WordPress's Plural_Forms for every GlotPress locale's plural expression, plus edge cases, into
 * plural-forms.json.
 *
 * Usage: .php-build/buildroot/bin/php record-plural-forms.php <wordpress-root> <glotpress-locales.php>
 *
 * locales.php is GlotPress's `locales/locales.php` (https://github.com/GlotPress/GlotPress), the list WordPress's own
 * pluralForms test reads.
 */

[$_, $root, $locales_file] = $argv + [null, null, null];
require rtrim($root, '/') . '/wp-includes/pomo/plural-forms.php';
require $locales_file;

$expressions = array();
foreach (GP_Locales::locales() as $locale) {
    $expressions[$locale->plural_expression] = true;
}
foreach (array(
    'n != 1', 'n ? 1 : 2', 'n > 1 ? 1 : 2', 'n > 1 ? n > 2 ? 1 : 2 : 3', '(n > 1) == 1', '(n > 1) < 2', 'n % 0', '%',
    'n # 2', 'n & 1', '((n)', '(n))', 'n : 2', 'n ? 1', 'n n', '!n', 'n == 1 || n == 2', 'n % 10 >= 2 && n % 10 <= 4',
    '99999999999999999999 % 7', 'n%100==1 ? 0 : n%100==2 ? 1 : 2', ' ( n ) ', "\tn\t!=\t1", 'n =! 1', 'n >== 1',
) as $e) {
    $expressions[$e] = true;
}
$ns = array_merge(range(-3, 200), range(1000, 1010), array(PHP_INT_MAX, PHP_INT_MIN));

$out = array();
foreach (array_keys($expressions) as $expression) {
    $expression = (string) $expression; // Array keys like "0" come back as integers.
    $entry = array('expression' => $expression);
    try {
        $p = new Plural_Forms($expression);
    } catch (Throwable $e) {
        $entry['error'] = get_class($e) . ': ' . $e->getMessage();
        $out[] = $entry;
        continue;
    }
    $values = array();
    foreach ($ns as $n) {
        try {
            $values[] = array($n, $p->get($n));
        } catch (Throwable $e) {
            $values[] = array($n, get_class($e) . ': ' . $e->getMessage());
        }
    }
    $entry['values'] = $values;
    $out[] = $entry;
}
file_put_contents(__DIR__ . '/plural-forms.json', json_encode($out, JSON_UNESCAPED_SLASHES) . "\n");
echo count($out), " expressions recorded\n";
