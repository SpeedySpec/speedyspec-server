<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.outside', static function (): void
{
    Fiber::suspend();
});

try
{
    Hook::run('fiber.outside');
}
catch (FiberError $e)
{
    echo $e->getMessage();
}
