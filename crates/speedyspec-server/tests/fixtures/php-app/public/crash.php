<?php

declare(strict_types=1);

// The worker dies mid-request, before any output.
exec('kill -9 ' . getmypid());
sleep(5);
