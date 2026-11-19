<?php

declare(strict_types=1);

// The blog fixture's front controller: like the placeholder a build without PHP answers with.
http_response_code(503);
header('Retry-After: 30');
echo 'service unavailable';
