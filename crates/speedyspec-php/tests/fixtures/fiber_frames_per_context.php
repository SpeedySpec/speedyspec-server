<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

// The fiber's firing is resumed from inside a firing of the main context; each
// side's exception must reach its own caller.
Hook::add('fiber.inner', static function (): void
{
    Fiber::suspend();
    throw new RuntimeException('from-fiber');
});
$fiberCaught = 'nothing';
$fiber = new Fiber(static function () use (&$fiberCaught): void
{
    try
    {
        Hook::run('fiber.inner');
    }
    catch (RuntimeException $e)
    {
        $fiberCaught = $e->getMessage();
    }
});
$fiber->start();

Hook::add('fiber.resumer', static function () use ($fiber): void
{
    $fiber->resume();
    throw new RuntimeException('from-main');
});
try
{
    Hook::run('fiber.resumer');
}
catch (RuntimeException $e)
{
    echo $e->getMessage(), ',';
}
echo $fiberCaught;
