<?php

declare(strict_types=1);

// $_SERVER, the query, the body and a cookie, as JSON.
header('Content-Type: application/json');
$keys = ['REQUEST_METHOD', 'QUERY_STRING', 'REQUEST_URI', 'SCRIPT_NAME', 'SCRIPT_FILENAME', 'PATH_INFO', 'HTTP_X_THING', 'REMOTE_ADDR', 'SERVER_NAME', 'SERVER_PORT', 'CONTENT_TYPE', 'CONTENT_LENGTH', 'GATEWAY_INTERFACE', 'DOCUMENT_ROOT'];
$server = [];
foreach ($keys as $key) {
    $server[$key] = $_SERVER[$key] ?? null;
}
echo json_encode(['server' => $server, 'get' => $_GET, 'body' => file_get_contents('php://input'), 'cookie' => $_COOKIE, 'memory_limit' => ini_get('memory_limit'), 'upload_max_filesize' => ini_get('upload_max_filesize'), 'content_url' => defined('WP_CONTENT_URL') ? WP_CONTENT_URL : null, 'content_dir' => defined('WP_CONTENT_DIR') ? WP_CONTENT_DIR : null, 'env' => getenv('SPEEDYSPEC_TEST_ENV') ?: null]);
