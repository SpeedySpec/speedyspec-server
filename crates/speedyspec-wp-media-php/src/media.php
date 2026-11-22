// Evaluated without an opening tag. No strict_types: WordPress's own files are not strict, so arguments coerce as they
// do there.

namespace SpeedySpec\WpMedia
{
    /**
     * Reads a file's audio or video metadata natively, as wp_read_audio_metadata() and wp_read_video_metadata() do.
     *
     *  A path through a stream wrapper (an offload plugin's s3://) is copied into a temporary file first, at most the
     *  reader's byte limit; a larger file is read as unreadable, as the native reader would treat it.
     *
     * @param string $kind
     *   "audio" or "video".
     * @param string $file
     *   The path WordPress was given.
     * @return array|false|mixed
     *   What the filter returned, or false when the file doesn't exist.
     */
    function read($kind, $file)
    {
        if (!file_exists($file)) {
            return false;
        }
        $file = (string) $file;
        if (!preg_match('#^[a-z][a-z0-9+.-]*://#i', $file) || stripos($file, 'file://') === 0) {
            return Native\read_metadata($kind, $file, $file, false);
        }
        $max = Native\max_bytes();
        $temp = tempnam(sys_get_temp_dir(), 'speedyspec-media-');
        $in = @fopen($file, 'rb');
        $out = $temp === false ? false : fopen($temp, 'wb');
        $copied = ($in && $out) ? stream_copy_to_stream($in, $out, $max + 1) : false;
        if ($in) {
            fclose($in);
        }
        if ($out) {
            fclose($out);
        }
        try {
            $too_large = $copied === false || $copied > $max;
            return Native\read_metadata($kind, $temp, $file, $too_large);
        } finally {
            if ($temp !== false) {
                @unlink($temp);
            }
        }
    }
}

namespace
{
    if (!function_exists('wp_read_audio_metadata')) {
        function wp_read_audio_metadata($file)
        {
            return \SpeedySpec\WpMedia\read('audio', $file);
        }
    }

    if (!function_exists('wp_read_video_metadata')) {
        function wp_read_video_metadata($file)
        {
            return \SpeedySpec\WpMedia\read('video', $file);
        }
    }

    if (!function_exists('wp_add_id3_tag_data')) {
        function wp_add_id3_tag_data(&$metadata, $data)
        {
            $metadata = \SpeedySpec\WpMedia\Native\wp_add_id3_tag_data($metadata, $data);
        }
    }

    if (!function_exists('wp_get_media_creation_timestamp')) {
        function wp_get_media_creation_timestamp($metadata)
        {
            return \SpeedySpec\WpMedia\Native\wp_get_media_creation_timestamp($metadata);
        }
    }
}
