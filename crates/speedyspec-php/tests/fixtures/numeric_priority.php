<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

// A numeric string priority sorts as its integer.
Hook::add('numeric.order', static fn () => print('b'), 20);
Hook::add('numeric.order', static fn () => print('a'), '5');
Hook::run('numeric.order');
echo '|';

// '20', ' 20', '-5' and 20, -5 name the same priorities.
$c = static fn () => print('x');
Hook::add('numeric.remove', $c, '20');
echo Hook::remove('numeric.remove', $c, 20) ? '1' : '0';
$d = static fn () => print('y');
Hook::add('numeric.remove', $d, 20);
echo Hook::remove('numeric.remove', $d, ' 20') ? '1' : '0';
Hook::add('numeric.remove', $d, '-5');
echo Hook::remove('numeric.remove', $d, -5) ? '1' : '0';
Hook::run('numeric.remove');
echo '|';

// Integral floats and float strings are their integer.
$e = static fn () => null;
Hook::add('numeric.float', $e, '20.0');
echo Hook::remove('numeric.float', $e, 20) ? '1' : '0';
Hook::add('numeric.float', $e, 20.0);
echo Hook::remove('numeric.float', $e, 20) ? '1' : '0';
echo '|';

Hook::add(
    'numeric.args',
    static function (): void
    {
        echo func_num_args();
    },
    10,
    '2',
);
Hook::run('numeric.args', 1, 2, 3);
echo '|';

foreach (['abc', '20abc', 20.5, null, '20.5', '', true, [20]] as $bad)
{
    try
    {
        Hook::add('numeric.bad', $e, $bad);
        echo 'accepted;';
    }
    catch (TypeError $t)
    {
        echo str_contains($t->getMessage(), '$priority') ? 'T' : 'wrong message: ' . $t->getMessage();
    }
}
echo '|';

try
{
    Hook::add('numeric.bad', $e, 10, -1);
}
catch (ValueError $v)
{
    echo str_contains($v->getMessage(), '$acceptedArgs') ? 'V' : 'wrong message';
}
try
{
    Hook::add('numeric.bad', $e, '99999999999999999999');
}
catch (TypeError $t)
{
    echo str_contains($t->getMessage(), '$priority') ? 'R' : 'wrong message';
}
try
{
    Hook::add('numeric.bad', $e, 10, 'two');
}
catch (TypeError $t)
{
    echo str_contains($t->getMessage(), '$acceptedArgs') ? 'A' : 'wrong message';
}
echo Hook::has('numeric.bad') ? 'y' : 'n';
