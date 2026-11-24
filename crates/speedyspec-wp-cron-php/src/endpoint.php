<?php
/**
 * SpeedySpec's cron endpoint: the path the server's cron scheduler calls (`php.cron`), in place of `wp-cron.php`.
 *
 * Only the server may call it. The server's internal calls carry no client address, and a release declaring this path
 * as `php.cron` makes it internal, so visitors get 404 from the server. The empty `REMOTE_ADDR` check is a second
 * guard for a release that does not declare it.
 *
 * - `budget` (seconds): how long the runner may run this site's due events.
 * - `site`, `host`, `path`: a follow-up call for one site of a network. WordPress boots as that site, with its plugins
 *   and theme, because its own multisite bootstrap reads `HTTP_HOST` and `REQUEST_URI` (as WP-CLI's `--url` sets
 *   them). If it boots a different site, such as through a domain mapping, the answer is 409.
 *
 * Answers JSON: the run's summary and, on a network's main call, `calls` (each other site with an event due now).
 */

if (($_SERVER['REMOTE_ADDR'] ?? '') !== '')
{
    http_response_code(404);
    exit;
}

$speedyspec_cron_site = isset($_GET['site']) ? (int) $_GET['site'] : 0;
if ($speedyspec_cron_site > 0)
{
    if (!isset($_GET['host'], $_GET['path']) || !is_string($_GET['host']) || !is_string($_GET['path']))
    {
        http_response_code(400);
        exit;
    }
    $_SERVER['HTTP_HOST']   = $_GET['host'];
    $_SERVER['REQUEST_URI'] = $_GET['path'];
}

ignore_user_abort(true);
define('DOING_CRON', true);

require __DIR__ . '/wp-load.php';

header('Content-Type: application/json; charset=utf-8');

if ($speedyspec_cron_site > 0 && get_current_blog_id() !== $speedyspec_cron_site)
{
    http_response_code(409);
    echo json_encode(array('error' => 'booted_another_site', 'site' => get_current_blog_id()));
    exit;
}

$speedyspec_cron_answer = \SpeedySpec\WpCron\run_due_events($_GET['budget'] ?? 30);
$speedyspec_cron_answer['calls'] = ($speedyspec_cron_site === 0 && is_multisite())
    ? \SpeedySpec\WpCron\due_sites()
    : array();

echo json_encode($speedyspec_cron_answer);
