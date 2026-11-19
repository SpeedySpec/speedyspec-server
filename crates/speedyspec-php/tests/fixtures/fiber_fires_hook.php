<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.plain', static fn (int $v): int => $v + 1);

$fiber = new Fiber(static function (): int
{
    $resumedWith = Fiber::suspend(Hook::filter('fiber.plain', 1));
    return Hook::filter('fiber.plain', $resumedWith);
});
$first = $fiber->start();
$fiber->resume(10);
echo $first, ',', $fiber->getReturn();
