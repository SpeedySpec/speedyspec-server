<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

$thrown = null;
Hook::add('exception.php', static function () use (&$thrown): void
{
    $thrown = new RuntimeException('boom');
    throw $thrown;
});

try
{
    Hook::run('exception.php');
    echo 'nothing thrown';
}
catch (RuntimeException $e)
{
    echo $e->getMessage();
    echo $e === $thrown ? '' : ' (not the object that was thrown)';
}
