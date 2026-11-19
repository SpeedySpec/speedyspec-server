<?php

declare(strict_types=1);

$fiber = new Fiber(static function (int $start): string
{
    $next = Fiber::suspend($start);
    $last = Fiber::suspend($next + 1);
    return "done:{$last}";
});
echo $fiber->start(1), ',';
echo $fiber->resume(1), ',';
$fiber->resume(3);
echo $fiber->getReturn();
