<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.throw', static function (): void
{
    Fiber::suspend();
});

$caught = null;
$fiber = new Fiber(static function () use (&$caught): void
{
    try
    {
        Hook::run('fiber.throw');
    }
    catch (RuntimeException $e)
    {
        $caught = $e;
    }
});
$fiber->start();
$thrown = new RuntimeException('x');
$fiber->throw($thrown);
echo $caught === $thrown ? 'same' : 'different';
