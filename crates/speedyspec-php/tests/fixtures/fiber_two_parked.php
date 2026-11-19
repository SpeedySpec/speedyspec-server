<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.two', static function (string $name): void
{
    echo "{$name}1";
    echo Fiber::suspend();
    echo "{$name}2";
});

$a = new Fiber(static function (): void
{
    Hook::run('fiber.two', 'A');
    echo 'A!';
});
$b = new Fiber(static function (): void
{
    Hook::run('fiber.two', 'B');
    echo 'B!';
});
$a->start();
$b->start();
echo '|';
$b->resume('-');
$a->resume('+');
