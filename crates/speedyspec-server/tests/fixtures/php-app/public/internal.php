<?php

declare(strict_types=1);

// Answered on the pool's internal worker.
header('Content-Type: text/plain');
echo 'internal ok ' . $_SERVER['REQUEST_METHOD'] . ' ' . ($_SERVER['SERVER_NAME'] ?? '') . ' ' . file_get_contents('php://input');
