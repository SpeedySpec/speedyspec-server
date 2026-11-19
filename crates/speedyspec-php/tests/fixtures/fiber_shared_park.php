<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

// Suspends only when fired inside a fiber, so the main context can fire the same hook.
Hook::add('fiber.shared', static function (): void
{
    echo 'x';
    if (Fiber::getCurrent() !== null)
    {
        echo Fiber::suspend('parked');
    }
});
Hook::add('fiber.shared', static function (): void
{
    echo 'y';
});

$GLOBALS['speedyspecParkedA'] = new Fiber(static function (): void
{
    echo '<';
    Hook::run('fiber.shared');
    echo '>';
});
echo $GLOBALS['speedyspecParkedA']->start();
