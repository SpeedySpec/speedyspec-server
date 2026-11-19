<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.inside', static function (): void
{
    $fiber = new Fiber(static function (): string
    {
        $x = Fiber::suspend('s');
        return "r{$x}";
    });
    echo $fiber->start(), ',';
    $fiber->resume('1');
    echo $fiber->getReturn(), ';';
});
Hook::run('fiber.inside');
echo 'done';
