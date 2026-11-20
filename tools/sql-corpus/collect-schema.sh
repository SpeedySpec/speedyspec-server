#!/bin/sh
# Collects WordPress's own schema (single site and the multisite global tables) as JSON lines.
# Usage: collect-schema.sh <wordpress-develop dir> <out.jsonl>
set -eu
dir=$1
out=$2
cd "$dir"
npm run --silent env:cli -- eval '
require_once ABSPATH . "wp-admin/includes/schema.php";
foreach ( array( "all", "ms_global" ) as $scope ) {
    foreach ( preg_split( "/;\s*\n/", wp_get_db_schema( $scope ) ) as $sql ) {
        $sql = trim( $sql );
        if ( "" !== $sql ) {
            echo json_encode( array( "sql" => $sql, "source" => "wordpress-schema" ), JSON_UNESCAPED_SLASHES ), "\n";
        }
    }
}' > "$out"
