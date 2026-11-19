<?php

declare(strict_types=1);

// The blog fixture's front controller: answers 503, served through the PHP runtime's pool.
http_response_code(503);
header('Retry-After: 30');
echo 'service unavailable';
