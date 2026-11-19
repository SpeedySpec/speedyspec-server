<?php

declare(strict_types=1);

echo implode(',', $argv), '|', $argc, '|', implode(',', $_SERVER['argv']), '|', $_SERVER['argc'];
