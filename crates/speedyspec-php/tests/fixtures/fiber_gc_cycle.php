<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('ctx.gc', static function (): void
{
    try
    {
        Fiber::suspend();
        echo 'resumed?';
    }
    finally
    {
        echo 'unwound;';
    }
});

// The holder and the fiber reference each other, so only the cycle collector can destroy the parked fiber.
$park = static function (): void
{
    $holder = new stdClass();
    $holder->fiber = new Fiber(static function () use ($holder): void
    {
        Hook::run('ctx.gc');
    });
    $holder->fiber->start();
};

$park();
echo 'out-of-scope;';
echo gc_collect_cycles() > 0 ? 'collected;' : 'nothing collected;';
Hook::ran('ctx.gc');

ob_start();
for ($i = 0; $i < 10; $i++)
{
    $park();
}
gc_collect_cycles();
Hook::ran('ctx.gc');
$before = memory_get_usage();
for ($i = 0; $i < 200; $i++)
{
    $park();
}
gc_collect_cycles();
Hook::ran('ctx.gc');
$grown = memory_get_usage() - $before;
ob_end_clean();
echo $grown;
