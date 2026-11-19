<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('filter_from_php', static fn (int $v): int => $v + 1);
echo Hook::filter('filter_from_php', 41);
