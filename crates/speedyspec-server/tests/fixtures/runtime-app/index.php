<?php
// Which runtime release this worker runs, and which release of the app.
echo (getenv('SPEEDYSPEC_RUNTIME_LABEL') ?: 'none') . ' ' . file_get_contents(__DIR__ . '/app.txt');
