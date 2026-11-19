<?php

declare(strict_types=1);

// The extensions WordPress needs to run inside the CLI's own PHP, and PDO's MySQL driver.
foreach (['mysqli', 'mysqlnd', 'pdo_mysql', 'dom', 'simplexml', 'xmlreader', 'xmlwriter', 'sodium', 'iconv', 'fileinfo', 'exif'] as $extension)
{
    echo extension_loaded($extension) ? 'y' : 'n';
}
echo class_exists(PDO::class) && in_array('mysql', PDO::getAvailableDrivers(), true) ? 'y' : 'n';
