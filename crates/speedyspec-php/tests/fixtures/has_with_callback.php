<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

$registered = static fn () => null;
$other = static fn () => null;
Hook::add('has_with_callback', $registered);
echo Hook::has('has_with_callback', $registered) ? 'y' : 'n';
echo Hook::has('has_with_callback', $other) ? 'y' : 'n';
echo Hook::has('has_with_callback', 'strlen') ? 'y' : 'n';
echo Hook::has('has_with_callback') ? 'y' : 'n';
