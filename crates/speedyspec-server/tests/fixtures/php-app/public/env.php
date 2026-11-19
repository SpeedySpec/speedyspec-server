<?php

declare(strict_types=1);

// What of the server's own environment reaches a worker: nothing but the allowlist and the app's own env.
echo getenv('SPEEDYSPEC_SERVER_ONLY') === false ? 'absent' : 'leaked';
echo ' ' . (getenv('SPEEDYSPEC_TEST_ENV') ?: 'none');
