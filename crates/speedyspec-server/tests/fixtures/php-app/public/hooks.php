<?php

declare(strict_types=1);

// Nothing a previous request registered may be seen here.
$before = (has_filter('fixture_hook') || has_filter('fatal_hook') || isset($GLOBALS['left_by_fatal'])) ? 'dirty' : 'clean';
add_filter('fixture_hook', fn ($v) => $v . '-x');
echo $before, '|', apply_filters('fixture_hook', 'v');
