<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

$o = new stdClass();
Hook::add('object_identity', static fn (object $v): object => $v);
Hook::add('object_identity', static fn (object $v): object => $v);
$r = Hook::filter('object_identity', $o);
echo spl_object_id($r) === spl_object_id($o) ? 'same' : 'different';
