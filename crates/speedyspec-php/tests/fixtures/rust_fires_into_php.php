<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('php.double', static fn (int $v): int => $v * 2);
