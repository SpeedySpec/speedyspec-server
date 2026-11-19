<?php

declare(strict_types=1);

use SpeedySpec\Hooks\Hook;

// The CLI defines no global hook functions, so code that brings its own (WordPress does)
// declares them without clashing, and SpeedySpec\Hooks\Hook is untouched by them.
function add_action(string $hook, callable $callback): string
{
    return 'mine';
}

function apply_filters(string $hook, mixed $value): mixed
{
    return $value;
}

echo add_action('own.action', static fn () => null), '|';
foreach (['add_filter', 'remove_action', 'remove_filter', 'has_action', 'has_filter', 'do_action', 'did_action'] as $name)
{
    echo function_exists($name) ? 'y' : 'n';
}
echo '|';
Hook::add('own.filter', static fn (int $value): int => $value + 1);
echo Hook::filter('own.filter', 1), apply_filters('own.filter', 1);
