<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

Hook::add('nested.exit', static function (): never
{
    exit(4);
});

try
{
    Hook::run('nested.exit');
}
finally
{
    echo 'not reached';
}
