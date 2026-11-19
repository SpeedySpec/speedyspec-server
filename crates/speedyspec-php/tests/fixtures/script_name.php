<?php

declare(strict_types=1);

$self = realpath($_SERVER['SCRIPT_NAME']) === __FILE__ ? 'self' : 'other';
echo $self, '|', $_SERVER['SCRIPT_FILENAME'] === $_SERVER['SCRIPT_NAME'] ? 'same' : 'differs', '|', $_SERVER['PHP_SELF'] === $_SERVER['SCRIPT_NAME'] ? 'same' : 'differs';
