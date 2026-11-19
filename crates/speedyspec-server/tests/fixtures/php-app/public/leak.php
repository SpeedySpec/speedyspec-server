<?php

declare(strict_types=1);

// Peaks well above a small memory_limit.
$big = str_repeat('x', 40 * 1024 * 1024);
echo strlen($big);
