<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('add_and_run', static fn () => print('a'));
Hook::run('add_and_run');
