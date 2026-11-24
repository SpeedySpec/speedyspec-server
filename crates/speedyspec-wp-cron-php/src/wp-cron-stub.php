<?php
/**
 * WordPress's `wp-cron.php`, replaced in a SpeedySpec release: the server's cron scheduler calls `speedyspec-cron.php`
 * instead, so visitors can no longer start a cron run.
 */

http_response_code(404);
