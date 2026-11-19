<?php

declare(strict_types=1);

// Five chunks, 300 ms apart.
for ($i = 0; $i < 5; $i++) {
    echo "chunk$i\n";
    flush();
    usleep(300000);
}
