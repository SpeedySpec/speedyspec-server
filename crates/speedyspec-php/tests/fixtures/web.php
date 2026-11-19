<?php

declare(strict_types=1);

http_response_code(201);
header('X-Test: one');
echo $_SERVER['REQUEST_METHOD'], '|', $_GET['a'] ?? '-', '|', file_get_contents('php://input'), '|', $_COOKIE['c'] ?? '-', '|', isset($GLOBALS['leftover']) ? 'dirty' : 'clean';
$GLOBALS['leftover'] = true;
