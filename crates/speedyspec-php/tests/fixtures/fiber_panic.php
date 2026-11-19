<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

$fiber = new Fiber(static function (): void
{
    Hook::run('rust.panics');
});
$fiber->start();
echo 'not reached';
