<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

/**
 * Reports when its last reference is released, so the test sees which firing frame held it.
 */
final class SpeedySpecFixtureNoisyException extends RuntimeException
{
    public function __destruct()
    {
        echo 'E-freed;';
    }
}

Hook::add('ctx.park', static function (): void
{
    Fiber::suspend();
    echo 'resumed;';
});
Hook::add('ctx.throw', static function (): never
{
    throw new SpeedySpecFixtureNoisyException('noisy');
});

$GLOBALS['speedyspecCtxFiber'] = new Fiber(static function (): void
{
    Hook::run('ctx.park');
});
$GLOBALS['speedyspecCtxFiber']->start();
