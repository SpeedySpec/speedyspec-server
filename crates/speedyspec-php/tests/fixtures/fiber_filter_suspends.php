<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.filter', static fn (int $v): int => $v + 1);
Hook::add('fiber.filter', static fn (int $v): int => $v * Fiber::suspend($v));
Hook::add('fiber.filter', static fn (int $v): int => $v + 100);

$fiber = new Fiber(static fn (): int => Hook::filter('fiber.filter', 1));
$seen = $fiber->start();
$fiber->resume(10);
echo $seen, ',', $fiber->getReturn();
