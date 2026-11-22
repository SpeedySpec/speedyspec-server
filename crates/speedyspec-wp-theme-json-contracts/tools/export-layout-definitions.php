<?php

declare(strict_types=1);

/**
 * Writes `wp_get_layout_definitions()` as PHP's `serialize()` output.
 *
 *  The contracts crate embeds the result (`src/schema/wordpress-<version>.layout`), so the layout table is
 *  WordPress's exact value. Run: `php tools/export-layout-definitions.php <wordpress root> > src/schema/…layout`.
 */

$root = rtrim($argv[1] ?? '', '/');
$file = "$root/wp-includes/block-supports/layout.php";
if ('' === $root || !is_file($file)) {
    fwrite(STDERR, "usage: php export-layout-definitions.php <wordpress root>\n");
    exit(1);
}
// Only the one function: layout.php also registers a block support at load time.
$tokens = token_get_all(file_get_contents($file));
$source = '';
foreach ($tokens as $i => $token) {
    if (is_array($token) && T_FUNCTION === $token[0] && 'wp_get_layout_definitions' === ($tokens[$i + 2][1] ?? '')) {
        $depth = 0;
        for ($k = $i; $k < count($tokens); $k++) {
            $text = is_array($tokens[$k]) ? $tokens[$k][1] : $tokens[$k];
            $source .= $text;
            if ('{' === $text) {
                $depth++;
            } elseif ('}' === $text && 0 === --$depth) {
                break;
            }
        }
        break;
    }
}
eval($source);
echo serialize(wp_get_layout_definitions());
