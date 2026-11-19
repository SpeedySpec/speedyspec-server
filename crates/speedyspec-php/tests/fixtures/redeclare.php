<?php

declare(strict_types=1);

/**
 * Declared twice so that compiling this file is a real engine fatal error, not an exception.
 */
function speedyspec_fixture_twice(): void
{
}

/**
 * The second declaration, which makes the compiler bail out.
 */
function speedyspec_fixture_twice(): void
{
}
