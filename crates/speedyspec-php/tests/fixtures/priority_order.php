<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('priority_order', static fn () => print('b'), 20);
Hook::add('priority_order', static fn () => print('a'), 5);
Hook::run('priority_order');
