<?php

declare(strict_types=1);

set_error_handler(static function (int $no, string $str): bool {
    echo $str;
    return true;
}, E_WARNING);
file_get_contents('/nonexistent/speedyspec');
