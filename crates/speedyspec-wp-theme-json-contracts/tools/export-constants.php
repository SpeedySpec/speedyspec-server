<?php

declare(strict_types=1);

/**
 * Writes `WP_Theme_JSON`'s class constants, in declaration order, as PHP's `serialize()` output.
 *
 *  The contracts crate embeds the result (`src/schema/wordpress-<version>.constants`), so the schema tables are
 *  WordPress's exact values. Run: `php tools/export-constants.php <wordpress root> > src/schema/…constants`.
 */

$root = rtrim($argv[1] ?? '', '/');
if ('' === $root || !is_file("$root/wp-includes/class-wp-theme-json.php")) {
    fwrite(STDERR, "usage: php export-constants.php <wordpress root>\n");
    exit(1);
}
require "$root/wp-includes/class-wp-theme-json.php";
echo serialize((new ReflectionClass('WP_Theme_JSON'))->getConstants());
