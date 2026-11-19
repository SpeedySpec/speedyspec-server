<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

$c = static fn () => print('x');
Hook::add('remove_closure', $c);
echo Hook::remove('remove_closure', $c) ? '1' : '0';
Hook::run('remove_closure');
