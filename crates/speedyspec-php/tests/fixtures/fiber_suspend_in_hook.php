<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.park', static function (): void
{
    echo 'a';
    $got = Fiber::suspend('parked');
    echo "b{$got}";
});
Hook::add('fiber.park', static function (): void
{
    echo 'c';
});

$fiber = new Fiber(static function (): void
{
    echo '[';
    Hook::run('fiber.park');
    echo ']';
});
echo $fiber->start(), '|main|';
$fiber->resume('1');
echo '|', $fiber->isTerminated() ? 'end' : 'running';
