<?php

declare(strict_types=1);

// The front controller: every path that is not a file.
echo 'front:', $_SERVER['REQUEST_URI'], '|', $_SERVER['SCRIPT_NAME'], '|', $_SERVER['PATH_INFO'] ?? '';
