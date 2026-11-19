<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

/**
 * Reports its own destruction, so the test sees the parked firing let go of its argument.
 */
final class SpeedySpecFixtureFiberArgument
{
    public function __destruct()
    {
        echo 'freed;';
    }
}

Hook::add('fiber.drop', static function (object $argument): void
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

$fiber = new Fiber(static function (object $argument): void
{
    try
    {
        Hook::run('fiber.drop', $argument);
        echo 'after hook?';
    }
    finally
    {
        echo 'fiber-finally;';
    }
});
$fiber->start(new SpeedySpecFixtureFiberArgument());
echo 'parked;';
unset($fiber);
gc_collect_cycles();
// Any native call releases what the unwound firing let go of.
Hook::ran('fiber.drop');
echo 'dropped;';

// Parking and dropping does not grow memory.
$park = static function (): void
{
    $fiber = new Fiber(static function (): void
    {
        Hook::run('fiber.drop', new stdClass());
    });
    $fiber->start();
};
ob_start();
for ($i = 0; $i < 10; $i++)
{
    $park();
}
Hook::ran('fiber.drop');
$before = memory_get_usage();
for ($i = 0; $i < 200; $i++)
{
    $park();
}
Hook::ran('fiber.drop');
$grown = memory_get_usage() - $before;
ob_end_clean();
echo $grown;
