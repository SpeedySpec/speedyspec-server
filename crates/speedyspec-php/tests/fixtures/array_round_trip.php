<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

$a = ['a' => 1, 5 => 'x', 'n' => [true, null]];
$b = Hook::filter('array.identity', $a);
echo var_export($a, true) === var_export($b, true) ? 'equal' : var_export($b, true);
