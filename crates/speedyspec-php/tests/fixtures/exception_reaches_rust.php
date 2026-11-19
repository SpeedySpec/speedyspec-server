<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('exception.rust', static function (): void
{
    throw new RuntimeException('boom');
});
