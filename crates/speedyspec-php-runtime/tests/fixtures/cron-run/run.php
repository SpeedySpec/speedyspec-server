<?php
require __DIR__ . '/boot.php';
echo json_encode(SpeedySpec\WpCron\run_due_events(30.0));
