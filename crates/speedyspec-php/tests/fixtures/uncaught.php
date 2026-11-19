<?php

declare(strict_types=1);

echo 'before;';
throw new RuntimeException('uncaught here');
