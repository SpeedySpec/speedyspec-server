<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

/**
 * Reports its own destruction, so the test sees when the last reference to it is gone.
 */
final class SpeedySpecFixtureTracked
{
    public function __destruct()
    {
        echo 'freed';
    }
}

Hook::add('refcount.php', static fn (object $v, mixed ...$rest): object => $v, 10, 3);

$o = new SpeedySpecFixtureTracked();
for ($i = 0; $i < 3; $i++)
{
    Hook::filter('refcount.php', $o, [$o], 'x');
    Hook::filter('refcount.rust', $o, [$o]);
}
echo 'before;';
unset($o);
echo ';after;';

// Warm up first, so the engine's allocator has settled before measuring.
$o = new stdClass();
for ($i = 0; $i < 10; $i++)
{
    Hook::filter('refcount.php', $o, [$o, 'two'], 3.5);
    Hook::filter('refcount.rust', $o, ['k' => $o]);
}
$before = memory_get_usage();
for ($i = 0; $i < 1000; $i++)
{
    Hook::filter('refcount.php', $o, [$o, 'two'], 3.5);
    Hook::filter('refcount.rust', $o, ['k' => $o]);
}
echo memory_get_usage() - $before;
