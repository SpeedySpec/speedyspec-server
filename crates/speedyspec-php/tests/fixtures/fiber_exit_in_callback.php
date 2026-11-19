<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.exit', static function (): never
{
    exit(6);
});
$fiber = new Fiber(static function (): void
{
    Hook::run('fiber.exit');
});
$fiber->start();
echo 'not reached';
