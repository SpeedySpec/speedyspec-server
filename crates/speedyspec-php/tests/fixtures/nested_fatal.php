<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('nested.fatal', static function (): void
{
    require __DIR__ . '/redeclare.php';
});
Hook::run('nested.fatal');
echo 'not reached';
