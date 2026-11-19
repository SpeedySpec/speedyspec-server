<?php

declare(strict_types=1);

// The CLI-era registry must not carry callbacks into the next request.
$before = \SpeedySpec\Hooks\Hook::has('legacy') ? 'dirty' : 'clean';
\SpeedySpec\Hooks\Hook::add('legacy', static fn () => null);
echo $before;
