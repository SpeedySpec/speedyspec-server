<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('fiber.shutdown', static function (): void
{
    try
    {
        Fiber::suspend();
    }
    finally
    {
        // Runs when PHP destroys the suspended fiber at shutdown.
        Hook::run('fiber.shutdown.seen');
    }
});
$GLOBALS['speedyspecParkedAtShutdown'] = new Fiber(static function (): void
{
    Hook::run('fiber.shutdown');
});
$GLOBALS['speedyspecParkedAtShutdown']->start();
echo $GLOBALS['speedyspecParkedAtShutdown']->isSuspended() ? 'parked' : 'not parked';
