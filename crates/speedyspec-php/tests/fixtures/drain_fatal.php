<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

/**
 * Fails fatally when destroyed, so the last release of it happens while Rust drains its references.
 */
final class SpeedySpecFixtureFatalOnDestruct
{
    public function __destruct()
    {
        require __DIR__ . '/redeclare.php';
    }
}

// The registry ignores an action's return value, so the bridge holds the last reference to this object.
Hook::add('drain.fatal', static fn (): object => new SpeedySpecFixtureFatalOnDestruct());
Hook::run('drain.fatal');
echo 'not reached';
