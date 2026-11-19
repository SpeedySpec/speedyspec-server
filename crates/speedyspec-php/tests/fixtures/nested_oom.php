<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('nested.oom', static function (): void
{
    $a = [];
    while (true)
    {
        $a[] = str_repeat('x', 1000000);
    }
});
Hook::run('nested.oom');
echo 'not reached';
