<?php

declare(strict_types=1);

$GLOBALS['left_by_fatal'] = true;
add_filter('fatal_hook', fn ($v) => $v);
undefined_function_in_the_fixture();
