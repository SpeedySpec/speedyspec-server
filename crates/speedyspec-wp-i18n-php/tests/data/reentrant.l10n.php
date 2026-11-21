<?php
// Outside the grammar the Rust reader parses (a function call), so PHP includes it; translating while it loads must
// work, as it does in WordPress.
$GLOBALS['speedyspec_reentry'] = __( 'baba', 'fast' );
return array_merge( array(), array( 'messages' => array( 'a' => 'b' ) ) );
