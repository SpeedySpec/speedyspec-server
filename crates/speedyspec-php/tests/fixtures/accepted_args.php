<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add(
    'accepted_args',
    static function (): void
    {
        echo func_num_args();
    },
    10,
    2,
);
Hook::run('accepted_args', 1, 2, 3);
