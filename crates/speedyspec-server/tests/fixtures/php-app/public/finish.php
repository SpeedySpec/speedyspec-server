<?php

declare(strict_types=1);

// Keeps running after the client goes away, as WP-Cron does.
ignore_user_abort(true);
usleep(800000);
file_put_contents(WP_CONTENT_DIR . '/finished', 'yes');
echo 'done';
