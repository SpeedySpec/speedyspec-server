<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;
use SpeedySpec\Hooks\HookException;

try
{
    Hook::filter('backend.down', 1);
    echo 'nothing thrown';
}
catch (HookException $e)
{
    echo $e instanceof RuntimeException ? $e->getMessage() : 'not a RuntimeException';
}
