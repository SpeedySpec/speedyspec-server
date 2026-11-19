<?php

declare(strict_types=1);

// 200 chunks of 64 KiB, recording progress in the site directory as it goes.
$progress = WP_CONTENT_DIR . '/progress';
for ($i = 1; $i <= 200; $i++) {
    echo str_repeat('x', 65536);
    flush();
    file_put_contents($progress, (string) $i);
}
